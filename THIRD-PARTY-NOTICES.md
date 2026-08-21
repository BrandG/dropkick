# Third-party notices

Dropkick itself is MIT licensed; see `LICENSE`. This file covers the
59 crates compiled into the binary, which carry their own licences.

Regenerate after any dependency change:

```
cargo metadata --format-version 1 --all-features
```

and check the tree against the rules with:

```
cargo deny check
```

## What Dropkick elects, and what it cannot

Most crates below offer a choice — `MIT OR Apache-2.0` means the person
distributing picks one. **Dropkick elects MIT wherever a choice is offered.**
The original expression is preserved in the table so that anyone redistributing
Dropkick can make a different election; electing Apache-2.0 would, for example,
give them that licence's express patent grant, at the cost of also carrying its
notice requirements.

Three obligations are not optional:

* **MIT** — 21 crates offer no alternative.
* **Zlib** — `foldhash` offers no alternative.
* **Unicode-3.0** — `unicode-ident` is `(MIT OR Apache-2.0) AND Unicode-3.0`.
  The `AND` is the point: Unicode-3.0 applies *in addition to* whichever of the
  first two is elected, not instead of one.

Nothing in the tree is copyleft. There is no GPL, LGPL, AGPL or MPL, so
distributing Dropkick carries no obligation to publish source, and no
share-alike condition attaches to Dropkick's own code.

## Dependencies

| crate | licence |
|---|---|
| `allocator-api2 0.2.21` | MIT OR Apache-2.0 |
| `bitflags 2.13.1` | MIT OR Apache-2.0 |
| `bytemuck 1.25.2` | Zlib OR Apache-2.0 OR MIT |
| `bytemuck_derive 1.12.0` | Zlib OR Apache-2.0 OR MIT |
| `calloop 0.14.4` | MIT |
| `calloop-wayland-source 0.4.1` | MIT |
| `cc 1.4.2` | MIT OR Apache-2.0 |
| `cfg-if 1.0.4` | MIT OR Apache-2.0 |
| `concurrent-queue 2.5.0` | Apache-2.0 OR MIT |
| `core_maths 0.1.1` | MIT |
| `crossbeam-utils 0.8.22` | MIT OR Apache-2.0 |
| `cursor-icon 1.2.0` | MIT OR Apache-2.0 OR Zlib |
| `downcast-rs 1.2.1` | MIT/Apache-2.0 |
| `equivalent 1.0.2` | Apache-2.0 OR MIT |
| `errno 0.3.14` | MIT OR Apache-2.0 |
| `find-msvc-tools 0.1.10` | MIT OR Apache-2.0 |
| `foldhash 0.1.5` | Zlib |
| `fontdue 0.9.4` | MIT OR Apache-2.0 OR Zlib |
| `hashbrown 0.15.5` | MIT OR Apache-2.0 |
| `hermit-abi 0.5.2` | MIT OR Apache-2.0 |
| `libc 0.2.189` | MIT OR Apache-2.0 |
| `libm 0.2.16` | MIT |
| `linux-raw-sys 0.12.1` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `log 0.4.33` | MIT OR Apache-2.0 |
| `memchr 2.8.3` | Unlicense OR MIT |
| `memmap2 0.9.11` | MIT OR Apache-2.0 |
| `pin-project-lite 0.2.17` | Apache-2.0 OR MIT |
| `pkg-config 0.3.33` | MIT OR Apache-2.0 |
| `polling 3.11.0` | Apache-2.0 OR MIT |
| `proc-macro2 1.0.107` | MIT OR Apache-2.0 |
| `quick-xml 0.41.0` | MIT |
| `quote 1.0.47` | MIT OR Apache-2.0 |
| `rustix 1.1.4` | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `shlex 2.0.1` | MIT OR Apache-2.0 |
| `slab 0.4.12` | MIT |
| `smallvec 1.15.2` | MIT OR Apache-2.0 |
| `smithay-client-toolkit 0.21.1` | MIT |
| `syn 3.0.3` | MIT OR Apache-2.0 |
| `thiserror 2.0.20` | MIT OR Apache-2.0 |
| `thiserror-impl 2.0.20` | MIT OR Apache-2.0 |
| `tracing 0.1.44` | MIT |
| `tracing-core 0.1.36` | MIT |
| `ttf-parser 0.25.1` | MIT OR Apache-2.0 |
| `unicode-ident 1.0.24` | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `wayland-backend 0.3.16` | MIT |
| `wayland-client 0.31.15` | MIT |
| `wayland-csd-frame 0.3.0` | MIT |
| `wayland-cursor 0.31.14` | MIT |
| `wayland-protocols 0.32.13` | MIT |
| `wayland-protocols-experimental 20251230.0.3` | MIT |
| `wayland-protocols-misc 0.3.12` | MIT |
| `wayland-protocols-wlr 0.3.12` | MIT |
| `wayland-scanner 0.31.11` | MIT |
| `wayland-sys 0.31.11` | MIT |
| `windows-link 0.2.1` | MIT OR Apache-2.0 |
| `windows-sys 0.61.2` | MIT OR Apache-2.0 |
| `xcursor 0.3.11` | MIT |
| `xkbcommon 0.8.0` | MIT |
| `xkeysym 0.2.1` | MIT OR Apache-2.0 OR Zlib |

## Licence texts

The texts below are those in force given the elections above. Apache-2.0 is not
reproduced: every Apache-2.0 crate here is dual-licensed and MIT is elected for
it. Anyone electing Apache-2.0 instead should include its text, available at
<https://www.apache.org/licenses/LICENSE-2.0>.

Copyright in each crate remains with its own authors, as recorded in that
crate's repository.

### MIT

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

### Zlib

This software is provided 'as-is', without any express or implied
warranty. In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.

### Unicode-3.0

Permission is hereby granted, free of charge, to any person obtaining a copy
of data files and any associated documentation (the "Data Files") or software
and any associated documentation (the "Software") to deal in the Data Files or
Software without restriction, including without limitation the rights to use,
copy, modify, merge, publish, distribute, and/or sell copies of the Data Files
or Software, and to permit persons to whom the Data Files or Software are
furnished to do so, provided that either (a) this copyright and permission
notice appear with all copies of the Data Files or Software, or (b) this
copyright and permission notice appear in associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF THIRD
PARTY RIGHTS. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN
THIS NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL
DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR
PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS
ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE
DATA FILES OR SOFTWARE.
