# Third-party notices

fleetz is MIT licensed (see `license` in `Cargo.toml`). It carries code derived
from the project below, whose licence requires its notice to travel with it.

## lazygit

<https://github.com/jesseduffield/lazygit>

`src/graph.rs` is a port of lazygit's commit-graph layout, from
`pkg/gui/presentation/graph/` at tag `v0.59.0` -- `graph.go` and `cell.go` for
the algorithm and the box-drawing table, and `graph_test.go` for nine of the
fixtures in that module's tests. The Rust is written from scratch and deviates
where noted in the source, but the pipe model, the lane-allocation rules and
the character table are lazygit's design, not ours.

The idea of computing the graph from parent hashes rather than parsing
`git log --graph`, which is what makes fleetz's history read one row per
commit, is theirs too.

```
MIT License

Copyright (c) 2018 Jesse Duffield

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
```
