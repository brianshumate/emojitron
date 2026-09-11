//! emojitron — print N emoji, by group or at random.

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use emojis::{Group, SkinTone};
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::SeedableRng;

/// Print emoji: random, or drawn from a standard Unicode emoji group.
#[derive(Parser)]
#[command(
    name = "emojitron",
    version,
    about = "Print N emoji, chosen randomly or from a standard emoji group",
    long_about = "emojitron prints emoji to standard output.\n\n\
        With no options it prints one emoji drawn uniformly at random from the \
        full Unicode emoji set, using the operating system's cryptographically \
        secure random number generator. Restrict the pool with --group, and ask \
        for more than one with the COUNT argument.",
    after_help = "EXAMPLES:\n  \
        emojitron                    Print one random emoji\n  \
        emojitron 10                 Print ten random emoji\n  \
        emojitron -g food 5          Print five food & drink emoji\n  \
        emojitron -g flags -u 20     Twenty distinct flags\n  \
        emojitron -n -g animals 3    Three animals, with names and shortcodes\n  \
        emojitron -t medium-dark 4   Four emoji with a medium-dark skin tone\n  \
        emojitron --seed 42 5        Five emoji, the same five every time\n  \
        emojitron --list-groups      Show the available group names"
)]
struct Cli {
    /// How many emoji to print
    #[arg(default_value_t = 1, value_name = "COUNT")]
    count: usize,

    /// Restrict the pool to one emoji group
    #[arg(short, long, value_name = "GROUP")]
    group: Option<GroupName>,

    /// Never repeat an emoji (fails/clamps if the pool is too small)
    #[arg(short, long)]
    unique: bool,

    /// Apply a skin tone to every emoji that supports one
    #[arg(short = 't', long, value_name = "TONE")]
    skin_tone: Option<Tone>,

    /// Seed the generator for reproducible output (not cryptographically secure)
    #[arg(long, value_name = "SEED")]
    seed: Option<u64>,

    /// Print the CLDR name and shortcode next to each emoji
    #[arg(short, long)]
    names: bool,

    /// String printed between emoji (default: a space, or a newline with --names)
    #[arg(short, long, value_name = "SEP")]
    separator: Option<String>,

    /// List the available group names and how many emoji each holds, then exit
    #[arg(short, long, exclusive = true)]
    list_groups: bool,
}

/// The nine standard Unicode CLDR emoji groups.
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum GroupName {
    #[value(alias = "smileys-and-emotion")]
    Smileys,
    #[value(alias = "people-and-body")]
    People,
    #[value(alias = "animals-and-nature")]
    Animals,
    #[value(alias = "food-and-drink")]
    Food,
    #[value(alias = "travel-and-places")]
    Travel,
    Activities,
    Objects,
    Symbols,
    Flags,
}

impl From<GroupName> for Group {
    fn from(g: GroupName) -> Self {
        match g {
            GroupName::Smileys => Group::SmileysAndEmotion,
            GroupName::People => Group::PeopleAndBody,
            GroupName::Animals => Group::AnimalsAndNature,
            GroupName::Food => Group::FoodAndDrink,
            GroupName::Travel => Group::TravelAndPlaces,
            GroupName::Activities => Group::Activities,
            GroupName::Objects => Group::Objects,
            GroupName::Symbols => Group::Symbols,
            GroupName::Flags => Group::Flags,
        }
    }
}

/// The five Fitzpatrick skin tone modifiers, plus the unmodified default.
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Tone {
    Default,
    Light,
    MediumLight,
    Medium,
    MediumDark,
    Dark,
}

impl From<Tone> for SkinTone {
    fn from(t: Tone) -> Self {
        match t {
            Tone::Default => SkinTone::Default,
            Tone::Light => SkinTone::Light,
            Tone::MediumLight => SkinTone::MediumLight,
            Tone::Medium => SkinTone::Medium,
            Tone::MediumDark => SkinTone::MediumDark,
            Tone::Dark => SkinTone::Dark,
        }
    }
}

/// Recolour an emoji, leaving it untouched when it takes no skin tone.
fn toned(e: &'static emojis::Emoji, tone: Option<Tone>) -> &'static emojis::Emoji {
    tone.and_then(|t| e.with_skin_tone(t.into())).unwrap_or(e)
}

fn pool(group: Option<GroupName>) -> Vec<&'static emojis::Emoji> {
    match group {
        Some(g) => Group::from(g).emojis().collect(),
        None => emojis::iter().collect(),
    }
}

fn warn(msg: &str) {
    let mut err = io::stderr();
    let (a, b) = if err.is_terminal() {
        ("\x1b[33mwarning:\x1b[0m ", "")
    } else {
        ("warning: ", "")
    };
    let _ = writeln!(err, "{a}{msg}{b}");
}

fn list_groups() {
    println!("{:<12} {:>5}  {}", "NAME", "COUNT", "CLDR GROUP");
    for (name, group) in [
        ("smileys", Group::SmileysAndEmotion),
        ("people", Group::PeopleAndBody),
        ("animals", Group::AnimalsAndNature),
        ("food", Group::FoodAndDrink),
        ("travel", Group::TravelAndPlaces),
        ("activities", Group::Activities),
        ("objects", Group::Objects),
        ("symbols", Group::Symbols),
        ("flags", Group::Flags),
    ] {
        println!(
            "{:<12} {:>5}  {:?}",
            name,
            group.emojis().count(),
            group
        );
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.list_groups {
        list_groups();
        return ExitCode::SUCCESS;
    }

    if cli.count == 0 {
        warn("COUNT is 0, nothing to print");
        return ExitCode::SUCCESS;
    }

    let pool = pool(cli.group);
    // Defensive: the emoji tables are non-empty, but never divide by zero.
    if pool.is_empty() {
        eprintln!("error: no emoji available in that group");
        return ExitCode::FAILURE;
    }

    // StdRng is a CSPRNG (ChaCha12) either way; --seed only makes the stream
    // reproducible, and predictable to anyone who knows the seed.
    let mut rng = match cli.seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_rng(&mut rand::rng()),
    };
    let picks: Vec<&emojis::Emoji> = if cli.unique {
        let want = if cli.count > pool.len() {
            warn(&format!(
                "only {} distinct emoji available, printing {} instead of {}",
                pool.len(),
                pool.len(),
                cli.count
            ));
            pool.len()
        } else {
            cli.count
        };
        pool.sample(&mut rng, want).copied().collect()
    } else {
        (0..cli.count)
            .map(|_| *pool.choose(&mut rng).expect("pool is non-empty"))
            .collect()
    };

    let picks: Vec<&emojis::Emoji> = picks.iter().map(|e| toned(e, cli.skin_tone)).collect();
    if cli.skin_tone.is_some() && picks.iter().all(|e| e.skin_tone().is_none()) {
        warn("none of the chosen emoji support a skin tone; printing them unmodified");
    }

    let sep = cli
        .separator
        .unwrap_or_else(|| if cli.names { "\n" } else { " " }.to_string());

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for (i, e) in picks.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, "{sep}");
        }
        let _ = if cli.names {
            write!(
                out,
                "{}  {}  :{}:",
                e.as_str(),
                e.name(),
                e.shortcode().unwrap_or("-")
            )
        } else {
            write!(out, "{}", e.as_str())
        };
    }
    let _ = writeln!(out);
    if out.flush().is_err() {
        // Broken pipe (e.g. `emojitron 1000 | head`) is not an error worth shouting about.
        return ExitCode::SUCCESS;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_and_groups() {
        assert!(pool(None).len() > 1000);
        // Every emoji in a group pool really belongs to that group.
        let food = pool(Some(GroupName::Food));
        assert!(!food.is_empty());
        assert!(food.iter().all(|e| e.group() == Group::FoodAndDrink));
        assert!(food.len() < pool(None).len());
    }

    #[test]
    fn unique_picks_do_not_repeat_and_clamp() {
        let p = pool(Some(GroupName::Flags));
        let mut rng = rand::rng();
        let picks: Vec<_> = p.sample(&mut rng, p.len() + 100).collect();
        assert_eq!(picks.len(), p.len(), "clamps to pool size");
        let mut s: Vec<_> = picks.iter().map(|e| e.as_str()).collect();
        s.sort_unstable();
        s.dedup();
        assert_eq!(s.len(), p.len(), "no repeats");
    }

    #[test]
    fn seed_is_reproducible() {
        let p = pool(None);
        let draw = |seed| -> Vec<&str> {
            let mut rng = StdRng::seed_from_u64(seed);
            (0..20)
                .map(|_| p.choose(&mut rng).unwrap().as_str())
                .collect()
        };
        assert_eq!(draw(42), draw(42));
        assert_ne!(draw(42), draw(43));
    }

    #[test]
    fn skin_tone_applies_only_where_supported() {
        let wave = emojis::get("\u{1F44B}").unwrap();
        let toned_wave = toned(wave, Some(Tone::Dark));
        assert_eq!(toned_wave.skin_tone(), Some(SkinTone::Dark));
        assert_ne!(toned_wave.as_str(), wave.as_str());

        // A tomato has no hands to recolour.
        let tomato = emojis::get("\u{1F345}").unwrap();
        assert_eq!(toned(tomato, Some(Tone::Dark)).as_str(), tomato.as_str());
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
