use shaderc::{self, ShaderKind, CompileOptions, TargetEnv, EnvVersion};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=resources/shaders");

    let compiler = shaderc::Compiler::new().unwrap();
    let mut options = CompileOptions::new().unwrap();
    options.set_target_env(TargetEnv::Vulkan, EnvVersion::Vulkan1_1 as u32);

    let out_dir = std::env::var_os("OUT_DIR").unwrap();

    {
        let source = std::fs::read_to_string("resources/shaders/ui.vert.glsl")?;
        let binary = compiler.compile_into_spirv(
            &source,
            ShaderKind::Vertex,
            "ui.vert.glsl",
            "main",
            Some(&options),
        )?;
        std::fs::write(std::path::Path::new(&out_dir).join("ui.vert.spv"), binary.as_binary_u8())?;
    }

    {
        let source = std::fs::read_to_string("resources/shaders/ui.frag.glsl")?;
        let binary = compiler.compile_into_spirv(
            &source,
            ShaderKind::Fragment,
            "ui.frag.glsl",
            "main",
            Some(&options),
        )?;
        std::fs::write(std::path::Path::new(&out_dir).join("ui.frag.spv"), binary.as_binary_u8())?;
    }

    Ok(())
}
