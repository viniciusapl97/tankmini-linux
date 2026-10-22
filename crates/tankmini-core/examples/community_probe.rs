//! Live check of the community client: `cargo run -p tankmini-core --features community --example community_probe -- marshall`
fn main() {
    let q = std::env::args().nth(1).unwrap_or_default();
    let (posts, total) = tankmini_core::community::search(&q, tankmini_core::community::Sort::Popular, 1, 10).unwrap();
    println!("{total} posts");
    for p in &posts {
        println!("{:>6} {:<40} {:>5}↓  {:?}", p.id, p.title.chars().take(40).collect::<String>(), p.downloads, p.files.iter().map(|f| (&f.display, f.kind)).collect::<Vec<_>>());
    }
    if let Some((p, f)) = posts.iter().find_map(|p| p.files.iter().find(|f| f.kind == tankmini_core::community::Kind::Amp).map(|f| (p, f))) {
        let b = tankmini_core::community::download(p.id, f).unwrap();
        println!("downloaded {} ({} bytes): {:?}", f.display, b.len(), tankmini_core::amp::Model::parse(&b).map(|m| m.name));
    }
}
