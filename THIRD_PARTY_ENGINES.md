# Third-party chess engines

Prince's Linux-Chess is licensed under GPL-3.0-or-later (see `LICENSE`). **Engines are separate
programs started as subprocesses; they are not part of this application and their licenses are not
this application's license.** The application contains no engine source code.

If you distribute a package that bundles engine binaries, you are responsible for including each
engine's license, copyright notice and (where required) corresponding source or a source offer.
Keep them under `engines/licenses/<engine>/` so the packaging scripts install them separately.

| Engine | License (as stated by the project at the time of writing — **verify the exact version before bundling**) |
|---|---|
| Stockfish | GPLv3 |
| Berserk | GPLv3 |
| pawnocchio | GPLv3 |
| PlentyChess | GPLv3 |
| Obsidian | GPLv3 |
| Reckless | AGPLv3 (network-use clause; read it before redistributing) |

The default packages do **not** bundle any engine; the `.deb` only *recommends* Ubuntu's `stockfish`
package. Network files (NNUE nets) may carry their own terms.
