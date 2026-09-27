use std::env;
use std::process;

/// Subcommands understood by the CLI.
pub const COMMANDS: &[&str] = &["build", "serve", "watch", "check", "lint", "fmt", "lsp"];

pub fn print_help() {
    eprintln!(
        "\
htmlang {} - a minimalist layout language that compiles to static HTML

Usage: htmlang [options] <file.hl | directory>
       htmlang <command> [args]

Commands:
  build <dir> [-o <out>] [--minify] [--strict]
                        Compile every .hl file under a directory, except
                        libraries (files that hold only @let definitions)
  serve [dir|file] [-p N] [--open]
                        Dev server with live reload
  watch [dir|file] [-o <out>]
                        Recompile on change, without a server
  check <file.hl | dir> [--format json]
                        Report diagnostics without writing output
  lint <file.hl | dir> [--format json]
                        Stricter checks (accessibility, nesting)
  fmt <file.hl>...      Format files in place
  lsp                   Start the language server (stdio)

Options:
  -o, --output <path>   Output file or directory
  -w, --watch           Recompile on change
  -s, --serve           Dev server with live reload (implies --watch)
  -p, --port <N>        Dev server port (default: 3000)
  --open                Open the browser (with --serve)
  -d, --dev             Development mode (readable output, source maps)
  -c, --check           Check for errors without writing output
  --strict              Treat warnings as errors
  --partial             Output an HTML fragment without the document wrapper
  --format json         Print diagnostics as JSON
  -h, --help            Show this help
  -V, --version         Show version

Examples:
  htmlang page.hl              Compile page.hl to page.html
  htmlang site/                Compile every .hl file in a directory
  htmlang -s --open site/      Serve a site and open the browser
  htmlang build src/ -o dist/  Compile src/ into dist/
  htmlang check src/           Check every file in a directory",
        env!("CARGO_PKG_VERSION")
    );
}

pub fn run_lsp() {
    let self_exe = env::current_exe().ok();
    let lsp_name = if cfg!(windows) {
        "htmlang-lsp.exe"
    } else {
        "htmlang-lsp"
    };

    let lsp_path = self_exe
        .as_ref()
        .and_then(|exe| exe.parent())
        .map(|dir| dir.join(lsp_name))
        .filter(|p| p.exists());

    let lsp_cmd = lsp_path
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| lsp_name.to_string());

    let status = std::process::Command::new(&lsp_cmd)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    match status {
        Ok(s) => {
            if !s.success() {
                process::exit(s.code().unwrap_or(1));
            }
        }
        Err(_) => {
            eprintln!("error: could not find htmlang-lsp binary");
            eprintln!(
                "hint: ensure htmlang-lsp is in the same directory as htmlang, or in your PATH"
            );
            process::exit(1);
        }
    }
}
