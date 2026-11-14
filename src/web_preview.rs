use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path};
use std::thread;

const DEFAULT_PORT: u16 = 23162;
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0";
const DEFAULT_PUBLIC_DIRECTORY: &str = "artifacts/north-seas-adventure/dist/public";

fn main() {
    let port = env::var("PORT")
        .ok()
        .map(|value| value.parse::<u16>())
        .transpose()
        .unwrap_or_else(|error| panic!("Некорректное значение PORT: {error}"))
        .unwrap_or(DEFAULT_PORT);
    let public_directory = env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_PUBLIC_DIRECTORY.to_owned());
    let bind_address = env::var("BIND_ADDRESS").unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned());
    let public_directory = fs::canonicalize(&public_directory).unwrap_or_else(|error| {
        panic!(
            "Не удалось открыть каталог веб-сборки {}: {error}",
            public_directory
        )
    });
    let listener = TcpListener::bind((bind_address.as_str(), port)).unwrap_or_else(|error| {
        panic!("Не удалось запустить веб-сервер на {bind_address}:{port}: {error}")
    });

    println!("Предпросмотр игры доступен по адресу {bind_address}:{port}.");
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let public_directory = public_directory.clone();
                thread::spawn(move || {
                    let _ = handle_connection(stream, &public_directory);
                });
            }
            Err(error) => eprintln!("Ошибка подключения к предпросмотру: {error}"),
        }
    }
}

fn handle_connection(mut stream: TcpStream, public_directory: &Path) -> io::Result<()> {
    let mut request_line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut request_line)?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let target = request_parts.next().unwrap_or_default();

    if !matches!(method, "GET" | "HEAD") {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
            method == "HEAD",
        );
    }
    if target.is_empty() {
        return write_response(
            &mut stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"Bad Request",
            method == "HEAD",
        );
    }

    let request_path = target.split('?').next().unwrap_or("/");
    let relative_path = request_path.trim_start_matches('/');
    let relative_path = if relative_path.is_empty() {
        Path::new("index.html")
    } else {
        Path::new(relative_path)
    };
    if relative_path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return write_response(
            &mut stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"Bad Request",
            method == "HEAD",
        );
    }

    let candidate = public_directory.join(relative_path);
    let candidate = if candidate.is_dir() {
        candidate.join("index.html")
    } else {
        candidate
    };
    let Ok(file_path) = fs::canonicalize(candidate) else {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not Found",
            method == "HEAD",
        );
    };
    if !file_path.starts_with(public_directory) {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not Found",
            method == "HEAD",
        );
    }

    let body = fs::read(&file_path)?;
    write_response(
        &mut stream,
        "200 OK",
        content_type(&file_path),
        &body,
        method == "HEAD",
    )
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    mime_type: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(body)?;
    }
    Ok(())
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::content_type;
    use std::path::Path;

    #[test]
    fn web_files_receive_browser_mime_types() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("game.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("game.wasm")), "application/wasm");
    }
}
