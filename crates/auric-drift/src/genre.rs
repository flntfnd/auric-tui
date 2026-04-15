use std::collections::HashMap;

pub struct GenreCompatibilityMatrix {
    matrix: HashMap<String, HashMap<String, f64>>,
}

impl GenreCompatibilityMatrix {
    pub fn new() -> Self {
        let mut m: HashMap<String, HashMap<String, f64>> = HashMap::new();

        let groups: &[(&[&str], f64)] = &[
            (&["rock", "alternative", "indie", "indie rock", "punk", "post-punk",
              "grunge", "garage rock", "psychedelic rock", "shoegaze"], 0.85),
            (&["metal", "heavy metal", "death metal", "black metal", "thrash metal",
              "doom metal", "progressive metal", "metalcore", "nu metal"], 0.85),
            (&["electronic", "house", "techno", "trance", "drum and bass", "dubstep",
              "ambient", "idm", "electro", "synthwave", "edm"], 0.8),
            (&["hip hop", "hip-hop", "rap", "trap", "grime", "boom bap"], 0.85),
            (&["r&b", "rnb", "soul", "neo soul", "motown", "funk", "disco"], 0.85),
            (&["jazz", "bebop", "fusion", "smooth jazz", "free jazz", "swing",
              "bossa nova", "latin jazz"], 0.8),
            (&["classical", "baroque", "romantic", "contemporary classical",
              "orchestral", "chamber music", "opera"], 0.8),
            (&["country", "americana", "bluegrass", "folk", "singer-songwriter",
              "acoustic"], 0.8),
            (&["pop", "synth pop", "dance pop", "electropop", "art pop",
              "dream pop", "indie pop", "power pop"], 0.85),
            (&["blues", "delta blues", "chicago blues", "blues rock"], 0.85),
            (&["reggae", "dub", "ska", "dancehall"], 0.85),
            (&["world", "afrobeat", "latin", "salsa", "cumbia", "samba",
              "flamenco", "celtic"], 0.75),
        ];

        let cross: &[(&str, &str, f64)] = &[
            ("rock", "metal", 0.7), ("rock", "blues", 0.75), ("rock", "pop", 0.65),
            ("rock", "country", 0.5), ("metal", "punk", 0.6), ("electronic", "pop", 0.6),
            ("electronic", "hip hop", 0.55), ("electronic", "ambient", 0.8),
            ("hip hop", "r&b", 0.8), ("hip hop", "pop", 0.55), ("r&b", "pop", 0.7),
            ("r&b", "jazz", 0.65), ("jazz", "blues", 0.7), ("jazz", "classical", 0.5),
            ("jazz", "soul", 0.7), ("folk", "indie", 0.65), ("folk", "country", 0.75),
            ("blues", "soul", 0.75), ("blues", "country", 0.55),
            ("reggae", "hip hop", 0.5), ("reggae", "world", 0.6),
            ("classical", "ambient", 0.45), ("country", "pop", 0.5),
            ("singer-songwriter", "indie", 0.7), ("singer-songwriter", "pop", 0.6),
            ("dream pop", "shoegaze", 0.9), ("dream pop", "ambient", 0.7),
            ("synthwave", "synth pop", 0.85), ("post-punk", "synth pop", 0.65),
            ("funk", "disco", 0.8), ("funk", "hip hop", 0.65),
            ("disco", "house", 0.75), ("soul", "gospel", 0.7),
        ];

        for (genres, intra) in groups {
            for a in *genres {
                for b in *genres {
                    if a != b {
                        m.entry(a.to_string()).or_default().insert(b.to_string(), *intra);
                    }
                }
            }
        }

        for (a, b, score) in cross {
            m.entry(a.to_string()).or_default().insert(b.to_string(), *score);
            m.entry(b.to_string()).or_default().insert(a.to_string(), *score);
        }

        Self { matrix: m }
    }

    pub fn score(&self, from: &str, to: &str) -> f64 {
        let a = from.to_lowercase();
        let b = to.to_lowercase();
        let a = a.trim();
        let b = b.trim();

        if a == b { return 1.0; }

        if let Some(direct) = self.matrix.get(a).and_then(|row| row.get(b)) {
            return *direct;
        }

        let a_tokens: Vec<&str> = a.split_whitespace().collect();
        let b_tokens: Vec<&str> = b.split_whitespace().collect();

        for (key, scores) in &self.matrix {
            let key_tokens: Vec<&str> = key.split_whitespace().collect();
            if a_tokens.iter().any(|t| key_tokens.contains(t)) {
                for (target, score) in scores {
                    let target_tokens: Vec<&str> = target.split_whitespace().collect();
                    if b_tokens.iter().any(|t| target_tokens.contains(t)) {
                        return score * 0.8;
                    }
                }
            }
        }

        0.4
    }
}

impl Default for GenreCompatibilityMatrix {
    fn default() -> Self { Self::new() }
}
