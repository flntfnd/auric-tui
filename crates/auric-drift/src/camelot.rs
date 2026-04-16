pub struct CamelotWheel;

struct CamelotPosition {
    number: i32,
    letter: u8,
}

const CAMELOT_MAP: [CamelotPosition; 24] = [
    CamelotPosition { number: 8, letter: b'B' },   // 0  C major
    CamelotPosition { number: 3, letter: b'B' },   // 1  C# major
    CamelotPosition { number: 10, letter: b'B' },  // 2  D major
    CamelotPosition { number: 5, letter: b'B' },   // 3  Eb major
    CamelotPosition { number: 12, letter: b'B' },  // 4  E major
    CamelotPosition { number: 7, letter: b'B' },   // 5  F major
    CamelotPosition { number: 2, letter: b'B' },   // 6  F# major
    CamelotPosition { number: 9, letter: b'B' },   // 7  G major
    CamelotPosition { number: 4, letter: b'B' },   // 8  Ab major
    CamelotPosition { number: 11, letter: b'B' },  // 9  A major
    CamelotPosition { number: 6, letter: b'B' },   // 10 Bb major
    CamelotPosition { number: 1, letter: b'B' },   // 11 B major
    CamelotPosition { number: 5, letter: b'A' },   // 12 C minor
    CamelotPosition { number: 12, letter: b'A' },  // 13 C# minor
    CamelotPosition { number: 7, letter: b'A' },   // 14 D minor
    CamelotPosition { number: 2, letter: b'A' },   // 15 Eb minor
    CamelotPosition { number: 9, letter: b'A' },   // 16 E minor
    CamelotPosition { number: 4, letter: b'A' },   // 17 F minor
    CamelotPosition { number: 11, letter: b'A' },  // 18 F# minor
    CamelotPosition { number: 6, letter: b'A' },   // 19 G minor
    CamelotPosition { number: 1, letter: b'A' },   // 20 Ab minor
    CamelotPosition { number: 8, letter: b'A' },   // 21 A minor
    CamelotPosition { number: 3, letter: b'A' },   // 22 Bb minor
    CamelotPosition { number: 10, letter: b'A' },  // 23 B minor
];

const KEY_NAMES: [&str; 24] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "Abm", "Am", "Bbm", "Bm",
];

impl CamelotWheel {
    pub fn compatibility(from: i32, to: i32) -> f32 {
        if !(0..24).contains(&from) || !(0..24).contains(&to) {
            return 0.5;
        }
        if from == to {
            return 1.0;
        }
        let a = &CAMELOT_MAP[from as usize];
        let b = &CAMELOT_MAP[to as usize];

        if a.number == b.number && a.letter != b.letter {
            return 0.95;
        }

        let diff = (a.number - b.number).abs();
        let wrapped = diff.min(12 - diff);

        if a.letter == b.letter {
            match wrapped {
                1 => 0.9,
                2 => 0.7,
                3 => 0.5,
                4 => 0.35,
                5 => 0.25,
                _ => 0.15,
            }
        } else {
            match wrapped {
                0 => 0.95,
                1 => 0.75,
                2 => 0.55,
                _ => 0.3,
            }
        }
    }

    pub fn name(key: i32) -> &'static str {
        if key >= 0 && (key as usize) < KEY_NAMES.len() {
            KEY_NAMES[key as usize]
        } else {
            "?"
        }
    }
}
