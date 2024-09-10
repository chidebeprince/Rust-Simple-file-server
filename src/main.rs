use std::{env, fs};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use walkdir::WalkDir;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

fn main() -> std::io::Result<()> {
    // Bind the TCP listener to the address and port
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Listening on 127.0.0.1:7878");

    for stream in listener.incoming() {
        let stream = stream?;
        handle_connection(stream)?;
    }

    Ok(())
}

fn handle_connection(mut stream: std::net::TcpStream) -> std::io::Result<()> {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer)?;

    // Parse the request line (naive implementation, just to extract the path)
    let request_line = String::from_utf8_lossy(&buffer);
    let path = decode_url(&parse_path(&request_line));
    println!("Request Path: {}", parse_path(&request_line)); // Raw path from the request
    println!("Decoded Path: {}", path);  

    // Check if the request is for favicon.ico
    if path == "/favicon.ico" {
        let response = "HTTP/1.1 404 Not Found\r\n\r\n";
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        return Ok(());
    }

    // Prevent backtracking
    if !is_valid_path(&path)? {
        let response = "HTTP/1.1 403 Forbidden\r\n\r\nBacktracking not allowed.";
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        return Ok(());
    }
    // Generate and send the HTML response
    generate_html_response(&path, &mut stream)?;

    Ok(())
}


fn parse_path(request: &str) -> String {
    // Extract the requested path from the HTTP request
    let lines: Vec<&str> = request.lines().collect();
    if lines.is_empty() {
        return "/".to_string();
    }
    let request_line = lines[0];
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else {
        "/".to_string()
    }
}

fn decode_url(url: &str) -> String {
    percent_encoding::percent_decode_str(url).decode_utf8_lossy().to_string()
}

// Prevent backtracking beyond the server's root directory
fn is_valid_path(requested_path: &str) -> std::io::Result<bool> {
    let rootcwd = std::env::current_dir()?;
    let rootcwd_len = rootcwd.canonicalize()?.components().count();
    
    let resource = rootcwd.join(&requested_path.trim_start_matches('/'));
    let resource_len = resource.canonicalize()?.components().count();
    
    println!("rootcwd: {}", rootcwd.display());
    println!("resource: {}", resource.display());
    
    Ok(rootcwd_len <= resource_len)
}

// Define the set of characters to encode
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');

fn generate_html_response(path: &str, stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    let mut html = r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
</head>
<body>
"#.to_string();

    // Define the root directory (absolute path to the files directory)
    let root_dir = Path::new(".");

    // Join the requested path with the root directory
    let base_path = root_dir.join(path.trim_start_matches('/'));
    println!("Base Path: {:?}", base_path);
    
    // Check if the file or directory exists
    if !base_path.exists() {
        println!("Error: File or directory does not exist: {}", base_path.display());
        let response = "HTTP/1.1 404 Not Found\r\n\r\nFile not found.";
        stream.write_all(response.as_bytes())?;
        return Ok(());
    }

    // Handle directory traversal
    if base_path.is_dir() {
        html.push_str(&format!("<h1>Currently in {}</h1>", base_path.display()));
        html.push_str("<ul>");

        // Allow going to the parent directory
        if let Some(parent) = base_path.parent() {
            let parent_path = parent.strip_prefix(root_dir).unwrap_or(parent);
            html.push_str(&format!(
                "<li><a href=\"{}\">Parent Directory</a></li>",
                utf8_percent_encode(parent_path.to_string_lossy().as_ref(), FRAGMENT)
            ));
        }

        // List all files and directories
        for entry in WalkDir::new(&base_path).max_depth(1) {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy();
            let file_path = entry.path().strip_prefix(root_dir).unwrap_or(entry.path());
            
            // Use `utf8_percent_encode` to encode file paths
            let encoded_path = utf8_percent_encode(file_path.to_string_lossy().as_ref(), FRAGMENT).to_string();

            html.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>",
                encoded_path, file_name
            ));
        }

        html.push_str("</ul></body></html>");

        // Return the HTTP response for a directory
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n{}",
            html
        );

        stream.write_all(response.as_bytes())?;
        stream.flush()?;
    } else {
        // Handle file serving
        let content_type = get_content_type(&base_path);
        println!("Serving file with content type: {}", content_type);

        // Serve file content
        let mut file_content = Vec::new();
        fs::File::open(&base_path)?.read_to_end(&mut file_content)?;

        // Return the HTTP response for the file
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\n\r\n",
            content_type
        );

        stream.write_all(response.as_bytes())?;
        stream.write_all(&file_content)?;
        stream.flush()?;
    }

    Ok(())
}



fn get_content_type(path: &Path) -> String {
    println!("Attempting to access: {}", path.display());
    // Use the `infer` crate to detect the file type
    if let Ok(mut file) = fs::File::open(path) {
        let mut buffer = [0u8; 512];
        if let Ok(_) = file.read(&mut buffer) {
            if let Some(kind) = infer::get(&buffer) {
                return kind.mime_type().to_string();
            }
        }
    }

    // If infer fails, fall back to file extension
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html".to_string(),
        Some("txt") => "text/plain".to_string(),
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("svg") => "image/svg+xml".to_string(),
        Some("mp4") => "video/mp4".to_string(),
        Some("mp3") => "audio/mpeg".to_string(),
        Some("json") => "application/json".to_string(),
        Some("pdf") => "application/pdf".to_string(),
        Some("md") => "text/markdown".to_string(),
        _ => "application/octet-stream".to_string(),  // Default to binary if unknown
    }
}