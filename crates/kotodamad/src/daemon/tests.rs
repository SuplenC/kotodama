use std::cmp::Ordering;

use super::*;

/// The order as a chain of comparisons, key by key, which
/// [`Candidate::rank_key`] packs into one number.
fn compared(a: &Candidate, b: &Candidate) -> Ordering {
    a.standing
        .cmp(&b.standing)
        .then_with(|| b.class.cmp(&a.class))
        .then_with(|| b.tier.cmp(&a.tier))
        .then_with(|| b.frecency.total_cmp(&a.frecency))
        .then_with(|| b.fine.cmp(&a.fine))
        .then_with(|| a.by_alias.cmp(&b.by_alias))
        .then_with(|| a.kind.cmp(&b.kind))
        .then_with(|| b.score.cmp(&a.score))
        .then_with(|| a.len.cmp(&b.len))
        .then_with(|| a.source.cmp(&b.source))
}

/// A xorshift generator: the same Candidates on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[self.below(from.len() as u64) as usize]
    }
}

const TIERS: [Tier; 7] = [
    Tier::NearMiss,
    Tier::Scattered,
    Tier::Inside,
    Tier::WordStart,
    Tier::Initials,
    Tier::Keyword,
    Tier::Exact,
];

/// A Candidate with every field drawn from a few values, so most pairs tie
/// on several keys and the later ones decide. `frecency` draws its
/// Frecency.
fn candidate(random: &mut Random, frecency: impl Fn(&mut Random) -> f64) -> Candidate {
    let mut candidate = Candidate::new(
        Source::Own(random.below(4) as u32),
        random.pick(&TIERS),
        random.pick(&[0, 1, 4, 254, 255]),
        random.pick(&[0, 1, 16, 17, 4095]),
        random.pick(&[0, 1, 5, 1023]),
    );
    candidate.standing = random.pick(&[Standing::Answer, Standing::Match, Standing::Fallback]);
    candidate.class = random.pick(&[Class::Rest, Class::Learned, Class::Named]);
    if random.below(2) == 0 {
        candidate.tier = rank::lift(candidate.tier);
    }
    candidate.by_alias = random.below(2) == 0;
    candidate.frecency = frecency(random);
    candidate.key = candidate.rank_key();
    candidate
}

/// A Frecency the key holds whole: none, a few picks' worth, or any number
/// with no more mantissa bits than the key keeps.
fn kept_frecency(random: &mut Random) -> f64 {
    match random.below(4) {
        0 => 0.0,
        1 => random.pick(&[0.5, 1.0, 5.0]),
        _ => {
            let dropped = 31 - key_bits::FRECENCY;
            let bits = ((random.next() as u32 & 0x7fff_ffff) >> dropped) << dropped;
            f64::from(f32::from_bits(bits.min(f32::MAX.to_bits())))
        }
    }
}

#[test]
fn the_rank_key_orders_as_the_keys_compared_one_by_one() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for _ in 0..200_000 {
        let a = candidate(&mut random, kept_frecency);
        let b = candidate(&mut random, kept_frecency);

        assert_eq!(a.rank(&b), compared(&a, &b), "{a:?}\n{b:?}");
    }
}

/// `candidate` as its key holds it: its Frecency at `f32` precision less
/// the mantissa bits the key drops, nucleo's score up to 4,095 and the
/// title's length up to 1,023.
fn as_kept(candidate: &Candidate) -> Candidate {
    let dropped = 31 - key_bits::FRECENCY;
    let bits = ((candidate.frecency as f32).to_bits() >> dropped) << dropped;
    Candidate {
        frecency: f64::from(f32::from_bits(bits)),
        score: candidate.score.min(4095),
        len: candidate.len.min(1023),
        ..*candidate
    }
}

#[test]
fn past_what_its_fields_hold_the_rank_key_orders_as_the_values_it_holds() {
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    let any = |random: &mut Random| f64::from_bits(random.next()).abs().min(1e9);
    for _ in 0..200_000 {
        let mut a = candidate(&mut random, any);
        let mut b = candidate(&mut random, any);
        a.score = random.next() as u16;
        b.len = random.next() as u16;
        a.key = a.rank_key();
        b.key = b.rank_key();

        assert_eq!(a.rank(&b), compared(&as_kept(&a), &as_kept(&b)), "{a:?}\n{b:?}");
    }
}
