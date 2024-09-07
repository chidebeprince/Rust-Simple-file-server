use std::fs;
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
    let path = parse_path(&request_line);

    // Generate and send the HTML response
    let response = generate_html_response(&path)?;
    stream.write_all(response.as_bytes())?;
    stream.flush()?;

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

// Define the set of characters to encode
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');

fn generate_html_response(path: &str) -> std::io::Result<String> {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html><html><body><ul>");

    // Walk through the directory and list all files and directories
    let base_path = Path::new(".").join(&path.trim_start_matches('/'));
    if base_path.is_dir() {
        for entry in WalkDir::new(base_path) {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy();
            
            // Use `utf8_percent_encode` to encode file paths
            let file_path = utf8_percent_encode(&entry.path().to_string_lossy(), FRAGMENT).to_string();

            html.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>",
                file_path, file_name
            ));
        }
    } else {
        let file_content = fs::read_to_string(base_path)?;
        html.push_str(&format!("<pre>{}</pre>", file_content));
    }

    html.push_str("</ul></body></html>");

    // Return the HTTP response
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n{}",
        html
    );

    Ok(response)
}