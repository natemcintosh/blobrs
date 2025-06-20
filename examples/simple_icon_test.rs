use std::env;

fn main() {
    println!("🔍 Terminal Icon Detection Test");
    println!("==============================");

    // Show current environment
    println!("\n📋 Environment Variables:");
    println!("TERM: {:?}", env::var("TERM"));
    println!("TERM_PROGRAM: {:?}", env::var("TERM_PROGRAM"));
    println!("LC_ALL: {:?}", env::var("LC_ALL"));
    println!("LC_CTYPE: {:?}", env::var("LC_CTYPE"));
    println!("LANG: {:?}", env::var("LANG"));
    println!("BLOBRS_ICONS: {:?}", env::var("BLOBRS_ICONS"));

    // Simple icon detection logic
    let icons = if env::var("BLOBRS_ICONS").unwrap_or_default() == "ascii" {
        "[DIR] [FILE] [LOADING] [ERROR] [OK]"
    } else if env::var("BLOBRS_ICONS").unwrap_or_default() == "minimal" {
        "D F * ! +"
    } else if is_unicode_capable() {
        "📁 📄 🔄 ❌ ✅"
    } else {
        "[DIR] [FILE] [LOADING] [ERROR] [OK]"
    };

    println!("\n🎭 Detected Icons: {}", icons);
}

fn is_unicode_capable() -> bool {
    // Simple detection
    if let Ok(term) = env::var("TERM") {
        term.contains("256color") || term.contains("kitty") || term.contains("alacritty")
    } else {
        false
    }
}
