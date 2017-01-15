ZMicro is a compression algorithm, which was specifically designed for the use in RACC.

ZMicro uses a standard bit-by-bit arithmetic encoder, but has an adaptive model.

The model consists of various submodels:

1. Prediction by partial matching with context order N.
2. Bit dependency table.

These are combined through adaptive context mixing, which works by weighting with the match rate. If model M reads a bit b=1 with prediction of being one P(b=1), then P(b=1) is added to the total sum. If b=0, P(b=0)=1-P(b=1) is added.
