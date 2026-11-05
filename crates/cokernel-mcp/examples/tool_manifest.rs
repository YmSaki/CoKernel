fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tools: Vec<_> = cokernel_mcp::contracts::all()
        .iter()
        .map(|c| c.tool())
        .collect();
    println!("{}", serde_json::to_string_pretty(&tools)?);
    Ok(())
}
