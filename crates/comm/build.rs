fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/comm.proto");
    prost_build::compile_protos(&["proto/comm.proto"], &["proto/"])?;
    Ok(())
}
