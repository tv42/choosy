# Sleigh -- `sled::Tree` with types

A sleigh is a heavier sled.
It is burdened with knowledge of types, and encoding/decoding of data.

There were libraries doing something like this, but as of this writing, they either were experiments, or lacked features I wanted.

Sleigh may or may not be spun into a feature-complete enough thing to justify its existence as a standalone project.
For now, we only wrap the `sled::Tree` methods we use.

See also https://github.com/spacejam/sled/issues/1266
