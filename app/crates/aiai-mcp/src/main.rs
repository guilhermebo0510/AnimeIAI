//! `aiai-mcp`: sem argumentos = servidor MCP (stdio); `call <AÇÃO> '<json>'` = uma chamada pela CLI. Ver `aiai_mcp::cli`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(aiai_mcp::cli::main_entry(args));
}
