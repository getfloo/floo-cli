"""Random app name generator."""

from __future__ import annotations

import random

ADJECTIVES = [
    "swift", "bright", "calm", "cool", "daring", "eager", "fair", "gentle",
    "happy", "keen", "lively", "mellow", "neat", "noble", "polite", "proud",
    "quick", "quiet", "rapid", "sharp", "sleek", "smart", "smooth", "snappy",
    "steady", "subtle", "sunny", "tender", "tidy", "vivid", "warm", "witty",
    "bold", "brave", "clean", "crisp", "epic", "fresh", "golden", "grand",
    "honest", "humble", "jolly", "kind", "light", "lucky", "mighty", "nifty",
    "olive", "plain", "prime", "royal", "rustic", "silent", "simple", "stable",
]

NOUNS = [
    "brook", "cedar", "cloud", "crest", "dawn", "delta", "dune", "ember",
    "fern", "fjord", "flame", "frost", "glade", "grove", "haven", "haze",
    "hill", "lake", "leaf", "marsh", "meadow", "mesa", "mist", "moss",
    "oak", "oasis", "orbit", "peak", "pine", "pond", "prairie", "rain",
    "reef", "ridge", "river", "sage", "shade", "shore", "spark", "spring",
    "stone", "storm", "stream", "summit", "tide", "trail", "vale", "wave",
    "willow", "wind", "birch", "canyon", "cliff", "coral", "creek", "field",
]


def generate_name() -> str:
    """Generate a random app name like 'swift-brook'."""
    adjective = random.choice(ADJECTIVES)
    noun = random.choice(NOUNS)
    return f"{adjective}-{noun}"
