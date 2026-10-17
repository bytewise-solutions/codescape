use anyhow::Result;
use clap::Parser;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(about = "Generate a large synthetic Gradle/Java project for scale tests")]
struct Args {
    out: PathBuf,
    #[arg(long, default_value_t = 1_000_000)]
    lines: usize,
    #[arg(long, default_value_t = 40)]
    modules: usize,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const PACKAGES_PER_MODULE: usize = 20;
const CLASSES_PER_PACKAGE: usize = 7;
const BIG_PACKAGE_CLASSES: usize = 2000;
const MAINS: usize = 10;

struct Class {
    module: usize,
    package: usize,
    name: String,
}

impl Class {
    fn fqn(&self) -> String {
        format!("com.synth.m{}.p{}.{}", self.module, self.package, self.name)
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let regular = args.modules * PACKAGES_PER_MODULE * CLASSES_PER_PACKAGE;
    let big_lines = BIG_PACKAGE_CLASSES * 40;
    let avg_lines = (args.lines.saturating_sub(big_lines) / regular.max(1)).max(30);

    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); args.modules];
    for (m, d) in deps.iter_mut().enumerate() {
        for _ in 0..(1 + rng.below(3)).min(m) {
            let target = m - 1 - rng.below(m.min(6));
            if !d.contains(&target) {
                d.push(target);
            }
        }
    }
    let mut classes: Vec<Class> = Vec::new();
    for m in 0..args.modules {
        for p in 0..PACKAGES_PER_MODULE {
            for c in 0..CLASSES_PER_PACKAGE {
                classes.push(Class { module: m, package: p, name: format!("C{m}x{p}x{c}") });
            }
        }
    }
    let big_start = classes.len();
    for c in 0..BIG_PACKAGE_CLASSES {
        classes.push(Class { module: 0, package: PACKAGES_PER_MODULE, name: format!("Big{c}") });
    }

    std::fs::create_dir_all(&args.out)?;
    let mut settings = String::from("rootProject.name = 'synth'\n");
    for m in 0..args.modules {
        writeln!(settings, "include 'm{m}'")?;
        let mut build = String::from("dependencies {\n");
        for d in &deps[m] {
            writeln!(build, "    implementation project(':m{d}')")?;
        }
        build.push_str("}\n");
        let dir = args.out.join(format!("m{m}"));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("build.gradle"), build)?;
    }
    std::fs::write(args.out.join("settings.gradle"), settings)?;

    let visible = |m: usize, deps: &Vec<Vec<usize>>| -> Vec<usize> {
        let mut seen = vec![m];
        let mut i = 0;
        while i < seen.len() {
            for &d in &deps[seen[i]] {
                if !seen.contains(&d) {
                    seen.push(d);
                }
            }
            i += 1;
        }
        seen
    };
    let module_classes: Vec<Vec<usize>> = (0..args.modules).map(|m| (0..big_start).filter(|&i| classes[i].module == m).collect()).collect();
    let mut total = 0usize;
    for (ci, class) in classes.iter().enumerate() {
        let big = ci >= big_start;
        let target_lines = if big { 40 } else { avg_lines / 2 + rng.below(avg_lines) };
        let pool: Vec<usize> = if big {
            (big_start..classes.len()).collect()
        } else {
            let vis = visible(class.module, &deps);
            let mut p: Vec<usize> = vis.iter().flat_map(|&m| module_classes[m].iter().copied()).collect();
            if rng.below(50) == 0 && class.module + 1 < args.modules {
                p.extend(module_classes[class.module + 1].iter().copied());
            }
            p
        };
        let fields: Vec<usize> = (0..3).map(|_| pool[rng.below(pool.len())]).filter(|&f| f != ci).collect();
        total += write_class(&args.out, class, &classes, &fields, target_lines, &mut rng)?;
    }
    let mains_written = (0..args.modules).filter(|m| *m >= args.modules - MAINS).count();
    for m in (args.modules - MAINS.min(args.modules))..args.modules {
        let dir = args.out.join(format!("m{m}/src/main/java/com/synth/m{m}/app"));
        std::fs::create_dir_all(&dir)?;
        let first = &classes[module_classes[m][0]];
        let src = format!(
            "package com.synth.m{m}.app;\n\nimport {};\n\npublic class Launcher{m} {{\n    public static void main(String[] args) {{\n        new {}().step0();\n    }}\n}}\n",
            first.fqn(),
            first.name
        );
        total += src.lines().count();
        std::fs::write(dir.join(format!("Launcher{m}.java")), src)?;
    }
    println!("wrote {} classes, {} mains, {total} lines into {}", classes.len(), mains_written, args.out.display());
    Ok(())
}

fn write_class(out: &Path, class: &Class, classes: &[Class], fields: &[usize], target_lines: usize, rng: &mut Rng) -> Result<usize> {
    let dir = out.join(format!("m{}/src/main/java/com/synth/m{}/p{}", class.module, class.module, class.package));
    std::fs::create_dir_all(&dir)?;
    let mut s = String::new();
    writeln!(s, "package com.synth.m{}.p{};\n", class.module, class.package)?;
    for &f in fields {
        writeln!(s, "import {};", classes[f].fqn())?;
    }
    writeln!(s, "\npublic class {} {{", class.name)?;
    for (i, &f) in fields.iter().enumerate() {
        writeln!(s, "    private {} f{i};", classes[f].name)?;
    }
    let mut method = 0;
    while s.lines().count() + 4 < target_lines {
        writeln!(s, "\n    public int step{method}() {{")?;
        writeln!(s, "        int acc = {method};")?;
        for _ in 0..6 {
            if !fields.is_empty() && rng.below(3) == 0 {
                let f = rng.below(fields.len());
                writeln!(s, "        acc += f{f}.step{}();", rng.below(3))?;
            } else {
                writeln!(s, "        acc = acc * 31 + {};", rng.below(1000))?;
            }
        }
        writeln!(s, "        return acc;\n    }}")?;
        method += 1;
    }
    if method == 0 {
        writeln!(s, "\n    public int step0() {{ return 0; }}")?;
    }
    writeln!(s, "}}")?;
    let lines = s.lines().count();
    std::fs::write(dir.join(format!("{}.java", class.name)), s)?;
    Ok(lines)
}
