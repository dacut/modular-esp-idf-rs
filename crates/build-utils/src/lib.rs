use {
    cc::Build,
    std::{
        collections::HashSet,
        env::{join_paths, split_paths, var_os},
        ffi::OsString,
        fs::{create_dir, File},
        io::Write,
        path::PathBuf,
    },
};

#[derive(Debug)]
pub struct ModBuild {
    pub build: Build,
    pub exported_include_dirs: Vec<OsString>,
    pub features: HashSet<String>,
}

impl Default for ModBuild {
    fn default() -> Self {
        let include_dir = Self::generated_include_dir();
        if !include_dir.exists() {
            create_dir(&include_dir).unwrap();
        }
        let mut build = Build::new();
        build.include(include_dir);

        let features = match var_os("CARGO_CFG_FEATURE") {
            None => HashSet::default(),
            Some(features) => features
                .to_str()
                .unwrap()
                .split(',')
                .map(|s| s.to_string())
                .collect(),
        };

        Self {
            build,
            exported_include_dirs: Vec::with_capacity(1),
            features,
        }
    }
}

impl ModBuild {
    pub fn out_dir() -> PathBuf {
        PathBuf::from(var_os("OUT_DIR").unwrap())
    }

    pub fn manifest_dir() -> PathBuf {
        PathBuf::from(var_os("CARGO_MANIFEST_DIR").unwrap())
    }

    pub fn generated_include_dir() -> PathBuf {
        Self::out_dir().join("include")
    }

    pub fn define(&mut self, key: impl AsRef<str>) {
        self.build.define(key.as_ref(), None);
    }

    pub fn define_value(&mut self, key: impl AsRef<str>, value: impl AsRef<str>) {
        self.build.define(key.as_ref(), Some(value.as_ref()));
    }

    pub fn mcu() -> String {
        if let Some(mcu) = var_os("CARGO_CFG_MCU") {
            return mcu.into_string().expect("MCU name is not valid UTF-8");
        }

        if let Some(target) = var_os("TARGET") {
            let target = target.into_string().expect("TARGET name is not valid UTF-8");
            if target.starts_with("xtensa-esp32-") {
                return "esp32".to_string();
            } else if target.starts_with("xtensa-esp32s2-") {
                return "esp32s2".to_string();
            } else if target.starts_with("xtensa-esp32s3-") {
                return "esp32s3".to_string();
            } else if target.starts_with("riscv32imc-") {
                return "esp32c3".to_string();
            }

            panic!("Unable to determine MCU from target triple: {target}");
        }

        panic!("Unable to determine MCU: --cfg mcu=<mcu> not passed to rustc and no target triple specified");
    }

    pub fn generate_sdkconfig(&mut self) -> PathBuf {
        let sdkconfig_filename = Self::generated_include_dir().join("sdkconfig.h");
        let mut sdkconfig_file = File::create(&sdkconfig_filename).unwrap();

        writeln!(sdkconfig_file, "#pragma once").unwrap();
        sdkconfig_file.flush().unwrap();
        drop(sdkconfig_file);

        sdkconfig_filename
    }

    pub fn include_dirs_to_path(&self) -> String {
        let path_var = join_paths(&self.exported_include_dirs).expect("One or more paths is not valid for a PATH-style environment variable");
        path_var.into_string().expect("The resulting PATH-style environment variable is not valid UTF-8")
    }

    pub fn add_library_include(&mut self, lib_name: impl AsRef<str>) {
        let lib_name = lib_name.as_ref().to_uppercase().replace('-', "_");
        let env_var = format!("DEP_{lib_name}_INCLUDE");
        let Some(includes) = var_os(&env_var) else {
            panic!("Environment variable {env_var} not set");
        };

        for path_el in split_paths(&includes) {
            self.build.include(path_el);
        }
    }

    pub fn add_component_source_files(&mut self, base_dir: impl AsRef<str>, component_src_files: &[&str]) {
        let component_base_dir = base_dir.as_ref().replace("${mcu}", &Self::mcu());
        let dir = Self::manifest_dir().join(&component_base_dir);
        for file in component_src_files {
            let file = file.replace("${mcu}", &Self::mcu());
            self.build.file(dir.join(&file));
            println!("cargo:rerun-if-changed={component_base_dir}/{file}");
        }
    }

    pub fn add_feature_component_source_files(&mut self, base_dir: impl AsRef<str>, sources: &[(&str, &[&str])]) {
        let base_dir = base_dir.as_ref();

        'feature_loop:
        for (feature_condition, files) in sources.iter() {
            for feature in feature_condition.split(',') {
                if let Some(feature) = feature.strip_prefix('!') {
                    if self.features.contains(feature) {
                        continue 'feature_loop;
                    }
                } else if !self.features.contains(feature) {
                    continue 'feature_loop;
                }
            }
            
            // All feature tests passed.
            self.add_component_source_files(base_dir, files)
        }
    }

    pub fn add_component_include_dirs(&mut self, base_dir: impl AsRef<str>, component_include_dirs: &[&str]) {
        let base_dir = base_dir.as_ref().replace("${mcu}", &Self::mcu());
        let dir = Self::manifest_dir().join(&base_dir);
        for include_dir in component_include_dirs {
            let include_dir = include_dir.replace("${mcu}", &Self::mcu());
            self.build.include(dir.join(&include_dir));
            println!("cargo:rerun-if-changed={base_dir}/{include_dir}");
            self.exported_include_dirs.push(OsString::from(dir.join(include_dir)));
        }
    }

    pub fn compile_library(&mut self, library_name: impl AsRef<str>) {
        let library_name = library_name.as_ref();
        self.build.compile(library_name);
        println!("cargo:rustc-link-lib=static={}", library_name);
        println!("cargo:INCLUDE={}", self.include_dirs_to_path());
    }
}