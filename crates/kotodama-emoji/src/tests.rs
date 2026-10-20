use super::*;

#[test]
fn every_line_is_a_character_with_a_name() {
    let mut seen = std::collections::HashSet::new();
    for character in Character::all() {
        assert!(!character.text.is_empty(), "{character:?}");
        assert!(!character.name.is_empty(), "{character:?}");
        assert!(seen.insert(character.text), "twice: {character:?}");
        let tones = character.tones.split(' ').filter(|t| !t.is_empty());
        assert!(matches!(tones.count(), 0 | 5), "{character:?}");
    }
}
