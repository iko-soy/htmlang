use std::env;
use std::process;

pub fn print_help() {
    eprintln!(
        "\
htmlang {} - a minimalist layout language that compiles to static HTML

Usage: htmlang [options] <file.hl | directory>

Commands:
  init [dir] [--template blog|docs|portfolio]
                        Create a new project (defaults to current directory)
  new <page-name>       Create a new .hl page from a template
  test [dir|file]       Run @assert directives across a project
  build <dir> [-o <out>] [--minify]  Compile all .hl files (parallel)
  serve [dir|file] [-p N] [--open]  Start dev server with hot reload
  watch [dir|file] [-o out]  Watch for changes and recompile
  check <file.hl | dir> Check for errors without writing output
  convert <file.html>   Convert an HTML file to .hl format (stdout)
  fmt <file.hl>         Format a .hl file (normalizes indentation)
  sitemap <dir>         Generate sitemap.xml from .hl files
  lint <file.hl | dir>  Stricter lint checks (accessibility, nesting, etc.)
  stats <file.hl | dir> Show file statistics (elements, CSS rules, colors)
  preview <file.hl>     Compile and open in browser (one-shot, no server)
  diff <a.hl> <b.hl>    Show differences between two .hl files
  export <dir> [-o f]   Compile and bundle into an archive
  repl                  Interactive REPL (type .hl, get HTML)
  feed <dir> [-b URL]   Generate RSS feed from @page metadata
  components <dir>      List all component definitions across a project
  deps <dir>            Show file dependency graph (@include/@import)
  dead-code <dir>       Find unused @let definitions across project
  deploy <dir> [--provider github-pages|netlify|vercel|cloudflare]
                        Build and deploy
  playground [out.html] Generate a self-contained HTML playground
  clean [dir]           Remove generated .html files
  upgrade [dir|file]    Auto-upgrade syntax to latest conventions
  create-component <name> [params...]  Scaffold a new component file
  outline <file.hl>     Show document structure tree
  doctor                Check toolchain health
  bundle <file|dir> [-o <out>]  Compile and inline all assets as data URIs
  size <file|dir>       Report output sizes (raw, minified, ~gzip)
  benchmark <file|dir>  Measure compile time and output size
  explain <file.hl>     Show what CSS each source line produces
  lsp                  Start the Language Server Protocol server
  completions <shell>  Generate shell completions (bash, zsh, fish)

Options:
  -w, --watch       Watch for changes and recompile
  -s, --serve       Start dev server with hot reload (implies --watch)
  -p, --port <N>    Port for dev server (default: 3000)
  --open            Open browser automatically (with --serve)
  -o, --output <path>  Output file/directory path
  -d, --dev         Development mode
  -c, --check       Check for errors without writing output
  --compat          Add vendor prefixes for broader browser support
  --strict          Treat warnings as errors (useful for CI)
  --partial         Output HTML fragment without document wrapper
  --format json     Output diagnostics as JSON to stdout
  -h, --help        Show this help
  -V, --version     Show version

Examples:
  htmlang init              Scaffold a new project
  htmlang init my-site      Scaffold in a new directory
  htmlang new about-us      Create about-us.hl from template
  htmlang page.hl           Compile page.hl to page.html
  htmlang site/             Compile all .hl files in directory
  htmlang -w page.hl        Recompile on file changes
  htmlang -s page.hl        Start dev server with hot reload
  htmlang -s --open site/   Serve and open browser
  htmlang -s site/          Serve a multi-page site
  htmlang -s -p 8080 page.hl
  htmlang -c page.hl        Lint without writing output
  htmlang check src/        Check all files in a directory
  htmlang convert page.html Convert HTML to .hl format
  htmlang --format json page.hl  Get diagnostics as JSON
  htmlang fmt page.hl       Format a file
  htmlang build src/ -o dist/  Compile all .hl files to dist/
  htmlang sitemap src/      Generate sitemap.xml
  htmlang lint src/         Lint all files in directory
  htmlang stats page.hl     Show file statistics",
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

pub fn print_shell_completions(shell: &str) {
    match shell {
        "bash" => print_bash_completions(),
        "zsh" => print_zsh_completions(),
        "fish" => print_fish_completions(),
        _ => {
            eprintln!("unknown shell: {}. Supported: bash, zsh, fish", shell);
            process::exit(1);
        }
    }
}

fn print_bash_completions() {
    println!(
        r#"_htmlang() {{
    local cur prev commands
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    commands="init new build serve watch check convert fmt sitemap lint stats preview diff export repl feed components deps dead-code deploy playground clean upgrade create-component outline doctor bundle size benchmark explain test lsp completions"

    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
        COMPREPLY+=( $(compgen -f -X '!*.hl' -- "$cur") )
        COMPREPLY+=( $(compgen -d -- "$cur") )
        return
    fi

    case "$prev" in
        -o|--output) COMPREPLY=( $(compgen -f -- "$cur") ) ;;
        -p|--port) COMPREPLY=() ;;
        --format) COMPREPLY=( $(compgen -W "json" -- "$cur") ) ;;
        --template|-t) COMPREPLY=( $(compgen -W "blog docs portfolio" -- "$cur") ) ;;
        --provider) COMPREPLY=( $(compgen -W "github-pages netlify vercel cloudflare" -- "$cur") ) ;;
        completions) COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") ) ;;
        *)
            COMPREPLY=( $(compgen -W "-w --watch -s --serve -d --dev -c --check --compat --strict --partial --open --format --minify -o --output -p --port -h --help -V --version" -- "$cur") )
            COMPREPLY+=( $(compgen -f -X '!*.hl' -- "$cur") )
            COMPREPLY+=( $(compgen -d -- "$cur") )
            ;;
    esac
}}
complete -F _htmlang htmlang"#
    );
}

fn print_zsh_completions() {
    println!(
        r#"#compdef htmlang

_htmlang() {{
    local -a commands=(
        'init:Create a new project'
        'new:Create a new .hl page'
        'build:Compile all .hl files'
        'serve:Start dev server'
        'watch:Watch and recompile'
        'check:Check for errors'
        'convert:Convert HTML to .hl'
        'fmt:Format a .hl file'
        'sitemap:Generate sitemap.xml'
        'lint:Lint checks'
        'stats:Show file statistics'
        'preview:Compile and open in browser'
        'diff:Compare two .hl files'
        'export:Bundle into archive'
        'repl:Interactive REPL'
        'feed:Generate RSS feed'
        'components:List component definitions'
        'deps:Show dependency graph'
        'dead-code:Find unused definitions'
        'deploy:Build and deploy'
        'playground:Generate playground'
        'clean:Remove generated files'
        'upgrade:Auto-upgrade syntax'
        'create-component:Scaffold component'
        'outline:Show document structure'
        'doctor:Check toolchain health'
        'bundle:Inline all assets'
        'size:Report output sizes'
        'benchmark:Measure compile time'
        'explain:Show CSS mapping per source line'
        'test:Run assertions'
        'lsp:Start LSP server'
        'completions:Generate shell completions'
    )

    local -a flags=(
        '-w[Watch for changes]'
        '--watch[Watch for changes]'
        '-s[Start dev server]'
        '--serve[Start dev server]'
        '-d[Development mode]'
        '--dev[Development mode]'
        '-c[Check only]'
        '--check[Check only]'
        '--compat[Vendor prefixes]'
        '--strict[Warnings as errors]'
        '--partial[Output HTML fragment]'
        '--open[Open browser]'
        '--minify[Minify output]'
        '-o[Output path]:path:_files'
        '--output[Output path]:path:_files'
        '-p[Port]:port:'
        '--port[Port]:port:'
        '--format[Output format]:format:(json)'
        '-h[Show help]'
        '--help[Show help]'
        '-V[Show version]'
        '--version[Show version]'
    )

    if (( CURRENT == 2 )); then
        _alternative 'commands:command:compadd -a commands' 'files:file:_files -g "*.hl"' 'dirs:directory:_directories'
    else
        _alternative 'flags:flag:compadd -a flags' 'files:file:_files -g "*.hl"' 'dirs:directory:_directories'
    fi
}}

_htmlang "$@""#
    );
}

fn print_fish_completions() {
    let commands = [
        ("init", "Create a new project"),
        ("new", "Create a new .hl page"),
        ("build", "Compile all .hl files"),
        ("serve", "Start dev server with hot reload"),
        ("watch", "Watch for changes and recompile"),
        ("check", "Check for errors"),
        ("convert", "Convert HTML to .hl"),
        ("fmt", "Format a .hl file"),
        ("sitemap", "Generate sitemap.xml"),
        ("lint", "Strict lint checks"),
        ("stats", "Show file statistics"),
        ("preview", "Compile and open in browser"),
        ("diff", "Compare two .hl files"),
        ("export", "Bundle into archive"),
        ("repl", "Interactive REPL"),
        ("feed", "Generate RSS feed"),
        ("components", "List component definitions"),
        ("deps", "Show dependency graph"),
        ("dead-code", "Find unused definitions"),
        ("deploy", "Build and deploy"),
        ("playground", "Generate playground"),
        ("clean", "Remove generated files"),
        ("upgrade", "Auto-upgrade syntax"),
        ("create-component", "Scaffold component"),
        ("outline", "Show document structure"),
        ("doctor", "Check toolchain health"),
        ("bundle", "Inline all assets as data URIs"),
        ("size", "Report output sizes"),
        ("benchmark", "Measure compile time"),
        ("explain", "Show CSS mapping per source line"),
        ("test", "Run assertions"),
        ("lsp", "Start LSP server"),
        ("completions", "Generate shell completions"),
    ];

    for (cmd, desc) in &commands {
        println!(
            "complete -c htmlang -n '__fish_use_subcommand' -a '{}' -d '{}'",
            cmd, desc
        );
    }

    let flags = [
        ("-w", "--watch", "Watch for changes"),
        ("-s", "--serve", "Start dev server"),
        ("-d", "--dev", "Development mode"),
        ("-c", "--check", "Check only"),
        ("", "--compat", "Add vendor prefixes"),
        ("", "--strict", "Treat warnings as errors"),
        (
            "",
            "--partial",
            "Output HTML fragment without document wrapper",
        ),
        ("", "--open", "Open browser"),
        ("", "--minify", "Minify output"),
        ("-o", "--output", "Output path"),
        ("-p", "--port", "Dev server port"),
        ("-h", "--help", "Show help"),
        ("-V", "--version", "Show version"),
    ];

    for (short, long, desc) in &flags {
        if short.is_empty() {
            println!(
                "complete -c htmlang -l '{}' -d '{}'",
                long.trim_start_matches('-'),
                desc
            );
        } else {
            println!(
                "complete -c htmlang -s '{}' -l '{}' -d '{}'",
                short.trim_start_matches('-'),
                long.trim_start_matches('-'),
                desc
            );
        }
    }

    println!("complete -c htmlang -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish'");
}
