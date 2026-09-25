use std::path::{Component, Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

const RELOAD_SCRIPT: &str =
    r#"<script>new EventSource("/_events").onmessage=()=>location.reload()</script>"#;

/// Serve a single HTML file (legacy single-file mode).
pub async fn run(port: u16, html_path: PathBuf, reload: broadcast::Sender<()>) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind to port {}: {}", port, e);
            return;
        }
    };

    loop {
        if let Ok((stream, _)) = listener.accept().await {
            let path = html_path.clone();
            let rx = reload.subscribe();
            tokio::spawn(async move {
                let _ = handle(stream, path, None, rx).await;
            });
        }
    }
}

/// Serve a directory of HTML files with auto-compilation, directory index, and route mapping.
pub async fn run_dir(port: u16, root_dir: PathBuf, reload: broadcast::Sender<()>) {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind to port {}: {}", port, e);
            return;
        }
    };

    loop {
        if let Ok((stream, _)) = listener.accept().await {
            let dir = root_dir.clone();
            let rx = reload.subscribe();
            tokio::spawn(async move {
                let _ = handle_dir_request(stream, dir, rx).await;
            });
        }
    }
}

async fn handle_dir_request<S>(
    mut stream: S,
    root_dir: PathBuf,
    reload_rx: broadcast::Receiver<()>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    if path == "/_events" {
        return handle_sse(stream, reload_rx).await;
    }

    let Some(requested) = resolve_request_path(&root_dir, path) else {
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return Ok(());
    };
    let clean_path = path.trim_start_matches('/');

    // Try to resolve the request to a file
    let file_path = if requested == root_dir {
        // Root: try index.html
        let index = root_dir.join("index.html");
        if index.exists() {
            Some(index)
        } else {
            // Generate directory listing
            let listing = generate_directory_listing(&root_dir);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
                listing.len(),
            );
            stream.write_all(header.as_bytes()).await?;
            stream.write_all(listing.as_bytes()).await?;
            return Ok(());
        }
    } else {
        // The file itself, then `<path>.html`, then `<path>/index.html`.
        [
            requested.clone(),
            requested.with_extension("html"),
            requested.join("index.html"),
        ]
        .into_iter()
        .find(|p| p.is_file())
    };

    // Reject symlinks that lead outside the served directory.
    let file_path = file_path.filter(|fp| is_within(&root_dir, fp));

    match file_path {
        Some(fp) => {
            let body = match std::fs::read(&fp) {
                Ok(content) => {
                    if fp.extension().is_some_and(|e| e == "html") {
                        inject_reload_script(&content)
                    } else {
                        content
                    }
                }
                Err(_) => {
                    stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                        .await?;
                    return Ok(());
                }
            };
            let content_type = content_type_for(&fp);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
                content_type,
                body.len(),
            );
            stream.write_all(header.as_bytes()).await?;
            stream.write_all(&body).await?;
        }
        None => {
            send_404(&mut stream, clean_path).await?;
        }
    }

    Ok(())
}

/// Generate a directory listing HTML page showing available .html pages.
fn generate_directory_listing(dir: &Path) -> String {
    let mut pages = Vec::new();
    collect_html_pages(dir, dir, &mut pages);
    pages.sort();

    let mut html = String::from(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>htmlang dev server</title>\
         <style>*{margin:0;box-sizing:border-box}body{font-family:system-ui,-apple-system,sans-serif;padding:2rem;max-width:600px;margin:0 auto}\
         h1{margin-bottom:1.5rem;font-size:1.5rem;color:#333}a{display:block;padding:0.6rem 1rem;margin:0.25rem 0;color:#2563eb;\
         text-decoration:none;border-radius:6px;transition:background 0.15s}a:hover{background:#f0f4ff}\
         .path{color:#888;font-size:0.85rem}</style></head><body>\
         <h1>htmlang dev server</h1>",
    );

    if pages.is_empty() {
        html.push_str("<p>No .html pages found. Compile some .hl files first.</p>");
    } else {
        for page in &pages {
            let display = page.trim_start_matches('/');
            let clean = display
                .strip_suffix("/index.html")
                .unwrap_or(display.strip_suffix(".html").unwrap_or(display));
            let label = if clean.is_empty() { "index" } else { clean };
            html.push_str(&format!(
                "<a href=\"{}\">{} <span class=\"path\">{}</span></a>",
                page, label, display
            ));
        }
    }

    html.push_str(RELOAD_SCRIPT);
    html.push_str("</body></html>");
    html
}

fn collect_html_pages(base: &Path, dir: &Path, pages: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_html_pages(base, &path, pages);
            } else if path.extension().is_some_and(|e| e == "html")
                && let Ok(rel) = path.strip_prefix(base)
            {
                pages.push(format!("/{}", rel.display()));
            }
        }
    }
}

async fn handle<S>(
    mut stream: S,
    html_path: PathBuf,
    root_dir: Option<&Path>,
    reload_rx: broadcast::Receiver<()>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    if path == "/_events" {
        handle_sse(stream, reload_rx).await
    } else {
        handle_file(stream, &html_path, root_dir, path).await
    }
}

async fn handle_sse<S>(mut stream: S, mut rx: broadcast::Receiver<()>) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let headers = "HTTP/1.1 200 OK\r\n\
                   Content-Type: text/event-stream\r\n\
                   Cache-Control: no-cache\r\n\
                   Connection: keep-alive\r\n\
                   X-Accel-Buffering: no\r\n\r\n";
    stream.write_all(headers.as_bytes()).await?;
    // Send retry hint (ms) so browsers reconnect quickly after a drop, plus an
    // initial comment line that flushes headers through proxies.
    if stream
        .write_all(b"retry: 1000\n: connected\n\n")
        .await
        .is_err()
    {
        return Ok(());
    }

    // Periodic keepalive comment lines are sent every 15s; intervening channel
    // events cause an immediate reload dispatch. `select!` multiplexes the two.
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
    heartbeat.tick().await; // consume immediate first tick

    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(()) => {
                    if stream.write_all(b"data: reload\n\n").await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = heartbeat.tick() => {
                // Comment line (starts with `:`) — keeps the connection alive
                // through proxies and surfaces dead peers as a write error.
                if stream.write_all(b": keepalive\n\n").await.is_err() {
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn handle_file<S>(
    mut stream: S,
    html_path: &Path,
    root_dir: Option<&Path>,
    request_path: &str,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let dir = root_dir.unwrap_or_else(|| {
        html_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    });
    let Some(requested) = resolve_request_path(dir, request_path) else {
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return Ok(());
    };
    let file_path = if requested == dir {
        html_path.to_path_buf()
    } else if is_within(dir, &requested) {
        requested
    } else {
        send_404(&mut stream, request_path).await?;
        return Ok(());
    };

    let body = match std::fs::read(&file_path) {
        Ok(content) => {
            if file_path.extension().is_some_and(|e| e == "html") {
                inject_reload_script(&content)
            } else {
                content
            }
        }
        Err(_) => {
            send_404(&mut stream, request_path).await?;
            return Ok(());
        }
    };

    let content_type = content_type_for(&file_path);
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
        body.len(),
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await?;

    Ok(())
}

/// Map a request target (`/a/b.css?v=2`) to a path under `root`. Strips the
/// query and fragment, percent-decodes, and rejects anything but a plain
/// relative path (`..`, absolute paths, drive prefixes). `/` maps to `root`.
fn resolve_request_path(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(path)?;
    let mut resolved = root.to_path_buf();
    for component in Path::new(decoded.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(resolved)
}

/// Whether `file`, with symlinks resolved, lies inside `root`.
fn is_within(root: &Path, file: &Path) -> bool {
    match (root.canonicalize(), file.canonicalize()) {
        (Ok(root), Ok(file)) => file.starts_with(root),
        _ => false,
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

async fn send_404<S>(stream: &mut S, path: &str) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let body = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>404</title>\
         <style>*{{margin:0;box-sizing:border-box}}body{{font-family:system-ui,-apple-system,sans-serif;\
         display:flex;align-items:center;justify-content:center;min-height:100vh;background:#fafafa;color:#333}}\
         .box{{text-align:center;padding:3rem}}\
         h1{{font-size:4rem;font-weight:200;color:#999;margin-bottom:0.5rem}}\
         p{{color:#666;margin-bottom:1.5rem}}\
         code{{background:#eee;padding:0.2em 0.5em;border-radius:4px;font-size:0.9rem}}\
         a{{color:#2563eb;text-decoration:none}}a:hover{{text-decoration:underline}}</style></head>\
         <body><div class=\"box\"><h1>404</h1><p>Not found: <code>{}</code></p>\
         <a href=\"/\">Back to index</a></div></body></html>",
        path.replace('<', "&lt;").replace('>', "&gt;")
    );
    let header = format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    Ok(())
}

fn inject_reload_script(content: &[u8]) -> Vec<u8> {
    let html = String::from_utf8_lossy(content);
    match html.rfind("</body>") {
        Some(pos) => {
            let mut result = String::with_capacity(html.len() + RELOAD_SCRIPT.len() + 1);
            result.push_str(&html[..pos]);
            result.push_str(RELOAD_SCRIPT);
            result.push('\n');
            result.push_str(&html[pos..]);
            result.into_bytes()
        }
        None => {
            let mut result = content.to_vec();
            result.extend_from_slice(RELOAD_SCRIPT.as_bytes());
            result
        }
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("eot") => "application/vnd.ms-fontobject",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") => "audio/ogg",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("xml") => "application/xml",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("wasm") => "application/wasm",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_paths_stay_inside_root() {
        let root = Path::new("/srv/site");
        assert_eq!(resolve_request_path(root, "/"), Some(root.to_path_buf()));
        assert_eq!(
            resolve_request_path(root, "/css/a.css?v=2#x"),
            Some(root.join("css/a.css"))
        );
        assert_eq!(
            resolve_request_path(root, "/my%20photo.png"),
            Some(root.join("my photo.png"))
        );
        // A doubled leading slash must not turn into an absolute path.
        assert_eq!(
            resolve_request_path(root, "//etc/passwd"),
            Some(root.join("etc/passwd"))
        );
        assert_eq!(resolve_request_path(root, "/../secret"), None);
        assert_eq!(resolve_request_path(root, "/a/%2e%2e/%2e%2e/secret"), None);
        assert_eq!(resolve_request_path(root, "/bad%zz"), None);
        assert_eq!(resolve_request_path(root, "é"), Some(root.join("é")));
    }
}
