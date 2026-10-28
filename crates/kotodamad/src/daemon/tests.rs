use std::cmp::Ordering;

use super::*;

/// The order as a chain of comparisons, key by key, which
/// [`Candidate::rank_key`] packs into one number.
fn compared(a: &Candidate, b: &Candidate) -> Ordering {
    a.standing
        .cmp(&b.standing)
        .then_with(|| b.class.cmp(&a.class))
        .then_with(|| b.points.cmp(&a.points))
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

const TIERS: [Tier; 8] = [
    Tier::NearMiss,
    Tier::Scattered,
    Tier::WeakKeyword,
    Tier::Inside,
    Tier::WordStart,
    Tier::Initials,
    Tier::Keyword,
    Tier::Exact,
];

/// A Candidate with every field drawn from a few values, so most pairs tie
/// on several keys and the later ones decide.
fn candidate(random: &mut Random) -> Candidate {
    let mut candidate = Candidate::new(
        Source::Own(random.below(4) as u32),
        random.pick(&TIERS),
        random.pick(&[0, 1, 4, 254, 255]),
        random.pick(&[0, 1, 16, 17, 4095]),
        random.pick(&[0, 1, 5, 1023]),
    );
    candidate.standing = random.pick(&[Standing::Answer, Standing::Match, Standing::Fallback]);
    candidate.class = random.pick(&[Class::Rest, Class::Learned, Class::Named]);
    candidate.by_alias = random.below(2) == 0;
    candidate.frecency = random.pick(&[0.0, 0.5, 1.0, 5.0, 40.0]);
    candidate.score(5.0, points::Boost::new(random.pick(&[0.5, 1.0, 2.0])).unwrap());
    candidate.key = candidate.rank_key();
    candidate
}

#[test]
fn the_rank_key_orders_as_the_keys_compared_one_by_one() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for _ in 0..200_000 {
        let a = candidate(&mut random);
        let b = candidate(&mut random);

        assert_eq!(a.rank(&b), compared(&a, &b), "{a:?}\n{b:?}");
    }
}

/// `candidate` as its key holds it: nucleo's score up to 4,095 and the
/// title's length up to 1,023.
fn as_kept(candidate: &Candidate) -> Candidate {
    Candidate {
        score: candidate.score.min(4095),
        len: candidate.len.min(1023),
        ..*candidate
    }
}

#[test]
fn past_what_its_fields_hold_the_rank_key_orders_as_the_values_it_holds() {
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    for _ in 0..200_000 {
        let mut a = candidate(&mut random);
        let mut b = candidate(&mut random);
        a.score = random.next() as u16;
        b.len = random.next() as u16;
        a.key = a.rank_key();
        b.key = b.rank_key();

        assert_eq!(a.rank(&b), compared(&as_kept(&a), &as_kept(&b)), "{a:?}\n{b:?}");
    }
}
