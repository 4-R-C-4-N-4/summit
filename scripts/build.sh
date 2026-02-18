use std::process::Command;
use std::path::Path;

fn main() {
    let web_dir = Path::new("../web");
    
    // Only rebuild web in release mode or if dist/ doesn't exist
    if !web_dir.join("dist").exists() || std::env::var("PROFILE").unwrap() == "release" {
        println!("cargo:warning=Building Astral web app...");
        
        // Install npm dependencies if node_modules doesn't exist
        if !web_dir.join("node_modules").exists() {
            println!("cargo:warning=Installing npm dependencies...");
            let npm_install = Command::new("npm")
                .args(&["install"])
                .current_dir(web_dir)
                .status()
                .expect("Failed to run npm install");
            
            if !npm_install.success() {
                panic!("npm install failed");
            }
        }
        
        // Build the React app
        let status = Command::new("npm")
            .args(&["run", "build"])
            .current_dir(web_dir)
            .status()
            .expect("Failed to build React app");
        
        if !status.success() {
            panic!("React build failed");
        }
        
        println!("cargo:warning=React app built successfully");
    }
    
    // Tell Cargo to re-run if web files change
    println!("cargo:rerun-if-changed=../web/src");
    println!("cargo:rerun-if-changed=../web/package.json");
    println!("cargo:rerun-if-changed=../web/vite.config.js");
}
