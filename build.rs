use shaderc::{self, ShaderKind, CompileOptions, TargetEnv, EnvVersion};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=resources/shaders");

    let mut glsl_files = vec![];

    for entry in std::fs::read_dir("resources/shaders")? {
        let path = entry?.path();
        if path.extension().unwrap() == "glsl" {
            glsl_files.push(path);
        }
    }

    let mut shaders = vec![];

    let compiler = shaderc::Compiler::new().unwrap();
    let mut options = CompileOptions::new().unwrap();
    options.set_target_env(TargetEnv::Vulkan, EnvVersion::Vulkan1_1 as u32);

    for file in glsl_files {
        let content = std::fs::read_to_string(&file)?;

        let kind = if file.ends_with("frag.glsl") {
            ShaderKind::Fragment
        } else {
            ShaderKind::Vertex
        };

        let result = compiler.compile_into_spirv(
            &content,
            kind,
            file.file_name().unwrap().to_str().unwrap(),
            "main",
            None,
        )?;

        shaders.push((file, result));
    }

    let out_dir = std::env::var_os("OUT_DIR").unwrap();

    for shader in shaders {
        let path = shader.0.with_extension("spv");
        let path = std::path::Path::new(&out_dir).join(path.file_name().unwrap());
        std::fs::write(path, shader.1.as_binary_u8())?;
    }

    Ok(())
}
