use rand::seq::SliceRandom;

const ADJECTIVES: &[&str] = &[
    "swift", "bright", "calm", "cool", "daring", "eager", "fair", "gentle", "happy", "keen",
    "lively", "mellow", "neat", "noble", "polite", "proud", "quick", "quiet", "rapid", "sharp",
    "sleek", "smart", "smooth", "snappy", "steady", "subtle", "sunny", "tender", "tidy", "vivid",
    "warm", "witty", "bold", "brave", "clean", "crisp", "epic", "fresh", "golden", "grand",
    "honest", "humble", "jolly", "kind", "light", "lucky", "mighty", "nifty", "olive", "plain",
    "prime", "royal", "rustic", "silent", "simple", "stable",
];

const NOUNS: &[&str] = &[
    "brook", "cedar", "cloud", "crest", "dawn", "delta", "dune", "ember", "fern", "fjord", "flame",
    "frost", "glade", "grove", "haven", "haze", "hill", "lake", "leaf", "marsh", "meadow", "mesa",
    "mist", "moss", "oak", "oasis", "orbit", "peak", "pine", "pond", "prairie", "rain", "reef",
    "ridge", "river", "sage", "shade", "shore", "spark", "spring", "stone", "storm", "stream",
    "summit", "tide", "trail", "vale", "wave", "willow", "wind", "birch", "canyon", "cliff",
    "coral", "creek", "field",
];

pub fn generate_name() -> String {
    let mut rng = rand::thread_rng();
    let adj = ADJECTIVES.choose(&mut rng).unwrap();
    let noun = NOUNS.choose(&mut rng).unwrap();
    format!("{adj}-{noun}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_name_format() {
        let name = generate_name();
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2, "name should be adjective-noun: {name}");
        assert!(
            ADJECTIVES.contains(&parts[0]),
            "invalid adjective: {}",
            parts[0]
        );
        assert!(NOUNS.contains(&parts[1]), "invalid noun: {}", parts[1]);
    }

    #[test]
    fn test_generate_name_not_empty() {
        let name = generate_name();
        assert!(!name.is_empty());
        assert!(name.contains('-'));
    }
}
