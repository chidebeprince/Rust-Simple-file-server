use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use url_escape::percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use walkdir::WalkDir;

fn main() -> std::io::Result<()> {
    // Specify the local directory to serve
    // "C:\\" and "/" are the roots for both windows and linux/MacOs respectively
    // Here:                          👇🏾👇🏾👇🏾👇🏾👇🏾👇🏾👇🏾👇🏾👇🏾
    let local_directory = Path::new(r"/workspaces/Rust-Simple-file-server");
    println!("Serving files from: {}", local_directory.display());

    // Bind the TCP listener to the address and port
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    println!("Listening on 127.0.0.1:7878");

    for stream in listener.incoming() {
        let stream = stream?;
        handle_connection(stream, &local_directory)?;
    }

    Ok(())
}

fn handle_connection(mut stream: std::net::TcpStream, base_directory: &Path) -> std::io::Result<()> {
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
    if !is_valid_path(&path, base_directory)? {
        let response = "HTTP/1.1 403 Forbidden\r\n\r\nBacktracking not allowed.";
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        return Ok(());
    }

    // Generate and send the HTML response
    generate_html_response(&path, base_directory, &mut stream)?;

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
    url_escape::percent_encoding::percent_decode_str(url).decode_utf8_lossy().to_string()
}

// Prevent backtracking beyond the server's base directory
fn is_valid_path(requested_path: &str, base_directory: &Path) -> std::io::Result<bool> {
    let base_dir_len = base_directory.canonicalize()?.components().count();
    
    let resource = base_directory.join(&requested_path.trim_start_matches('/'));
    let resource_len = resource.canonicalize()?.components().count();

    Ok(base_dir_len <= resource_len)
}

// Define the set of characters to encode
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');

fn generate_html_response(path: &str, base_directory: &Path, stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    let mut html = r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
</head>
<body>
"#.to_string();

    let base_path = base_directory.join(path.trim_start_matches('/'));

    // Traversing directories
    if base_path.is_dir() {
        html.push_str(&format!("<h1>Currently in {}</h1>", base_path.display()));

        // Always show "Up to previous directory" link
        let mut parent_path = path.to_string();
        if path != "/" {
            if let Some(pos) = parent_path.rfind('/') {
                parent_path.truncate(pos);
            }
            if parent_path.is_empty() {
                parent_path = "/".to_string();
            }
            html.push_str(&format!(
                "<li><a href=\"{}\">Up to previous directory</a></li>",
                utf8_percent_encode(&parent_path, FRAGMENT)
            ));
        } else {
            // In root directory, show "Up to previous directory" link pointing to root
            html.push_str(&format!(
                "<li><a href=\"{}\">Up to previous directory</a></li>",
                utf8_percent_encode("/", FRAGMENT)
            ));
        }

        // List all files and directories
        html.push_str("<ul>");
        for entry in WalkDir::new(&base_path).max_depth(1) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Error reading directory entry: {}", e);
                    continue; // Skip entry if there's an error
                }
            };
            let file_name = entry.file_name().to_string_lossy();

            // Handle `strip_prefix` with explicit error handling
            let file_path = match entry.path().strip_prefix(base_directory) {
                Ok(relative_path) => utf8_percent_encode(&relative_path.to_string_lossy(), FRAGMENT).to_string(),
                Err(e) => {
                    eprintln!("Error stripping prefix: {}", e);
                    continue; // Skip this entry on error
                }
            };

            // Add trailing slash for directories
            let display_name = if entry.path().is_dir() {
                format!("{}/", file_name)
            } else {
                file_name.to_string()
            };

            html.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>",
                file_path, display_name
            ));
        }
        html.push_str("</ul></body></html>");

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n{}",
            html
        );

        // Gracefully handle potential errors when writing the response
        if let Err(e) = stream.write_all(response.as_bytes()) {
            eprintln!("Failed to write HTML response: {}", e);
            return Err(e);  // Abort on write error
        }
        if let Err(e) = stream.flush() {
            eprintln!("Failed to flush stream: {}", e);
            return Err(e);  // Abort on flush error
        }
    } else {
        // Reading files and handling content-type
        let content_type = get_content_type(&base_path);

        // Serve file content with correct Content-Type
        let mut file_content = Vec::new();
        if let Err(e) = fs::File::open(&base_path)?.read_to_end(&mut file_content) {
            eprintln!("Failed to read file {}: {}", base_path.display(), e);
            return Err(e);  // Abort on file read error
        }

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\n\r\n",
            content_type
        );

        // Write header and handle errors
        if let Err(e) = stream.write_all(response.as_bytes()) {
            eprintln!("Failed to write file header: {}", e);
            return Err(e);  // Abort on write error
        }

        // Write file content and handle errors
        if let Err(e) = stream.write_all(&file_content) {
            eprintln!("Failed to write file content: {}", e);
            return Err(e);  // Abort on file write error
        }

        if let Err(e) = stream.flush() {
            eprintln!("Failed to flush stream after writing file content: {}", e);
            return Err(e);  // Abort on flush error
        }
    }

    Ok(())
}


fn get_content_type(path: &Path) -> String {
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
        Some("zip") => "application/zip".to_string(),
        Some("rs") => "text/plain".to_string(),
        Some("toml") => "text/plain".to_string(),
        Some("lock") => "text/plain".to_string(),
        Some("TAG") => "text/plain".to_string(),
        Some("HEAD") => "text/plain".to_string(),
        Some("mov") => "video/mp4".to_string(),
        _ => "application/octet-stream".to_string(),  // Default to binary if unknown
    }
}