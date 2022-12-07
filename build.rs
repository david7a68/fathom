use std::path::{Path, PathBuf};

use shaderc::ShaderKind;

const SHADER_DIR: &str = "resources/shaders/";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=resources/shaders");

    let compiler = Compiler::new(SHADER_DIR, std::env::var_os("OUT_DIR").unwrap());
    compiler.compile_shader("fill.vert.glsl");
    compiler.compile_shader("fill.frag.glsl");
    compiler.compile_shader("image_upload_uint.comp.glsl");

    Ok(())
}

struct Compiler {
    compiler: shaderc::Compiler,
    options: shaderc::CompileOptions<'static>,
    src_dir: PathBuf,
    dst_dir: PathBuf,
}

impl Compiler {
    const SHADER_KINDS: &[(&'static str, ShaderKind)] = &[
        ("vert.glsl", ShaderKind::Vertex),
        ("frag.glsl", ShaderKind::Fragment),
        ("comp.glsl", ShaderKind::Compute),
    ];

    fn new(src_dir: impl AsRef<Path>, dst_dir: impl AsRef<Path>) -> Self {
        let mut options = shaderc::CompileOptions::new().unwrap();
        options.set_target_env(
            shaderc::TargetEnv::Vulkan,
            shaderc::EnvVersion::Vulkan1_1 as u32,
        );

        Self {
            compiler: shaderc::Compiler::new().unwrap(),
            options,
            src_dir: src_dir.as_ref().to_owned(),
            dst_dir: dst_dir.as_ref().to_owned(),
        }
    }

    fn compile_shader(&self, name: &str) {
        let src_name = self.src_dir.join(name);

        let kind = Self::SHADER_KINDS
            .iter()
            .find_map(|(s, k)| name.ends_with(s).then_some(*k))
            .unwrap();

        println!("src_name: {:?}", src_name);

        let source = std::fs::read_to_string(&src_name).unwrap();
        let binary = self
            .compiler
            .compile_into_spirv(&source, kind, name, "main", Some(&self.options))
            .unwrap();

        let dst_name = {
            // to avoid an extra allocation
            let mut p = self.dst_dir.join(name);
            p.set_extension("spv");
            p
        };

        std::fs::write(dst_name, binary.as_binary_u8()).unwrap();
    }
}
