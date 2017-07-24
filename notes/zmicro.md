ZMicro is a compression algorithm, which was specifically designed for the use
in RACC.

ZMicro uses a standard bit-by-bit arithmetic encoder, but has an adaptive
model.

The model consists of various submodels:

- Prediction by partial matching with context congruence classes modulo N of order M.
- Average bit on same congruence class modulo N.
- Bit dependency table per N bits.
- Fifty-fifty (identity)
- Repeat last

These are combined through adaptive context mixing, which works by weighting
with the match rate.

If model M reads a bit b=1 with prediction of being one P(b=1), then P(b=1) is
the error. If b=0, P(b=0)=1-P(b=1) is the error.

Each model gets an accumulated error rate, which consists of a weighted sum of
the individual errors. For every new error, the sum is multiplied by some
number K≤1, called the cool-down factor, then the error is added.

Models are compared by ratio. For example model M1 is better than M2 by an
factor of M1/M2. In other words, a model M is judged on its ratio to the sum of
all error accumulations.

If a model rates a k'th (for some factor k called the exit factor) of the
accumulated error sum, it is dropped and it's error accumulation is subtracted
from the overall sum.
