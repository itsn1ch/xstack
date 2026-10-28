# Design section template

The `# Design` section of the epic's `spec.md` (or the design comment on a remote spec issue). It is the only design artifact. Keep it tight: one line per fact, no restating between parts, no boilerplate. Replace the italic notes with content. Place it right after the original spec and before the first `# Evolution` section. When an evolution changes the design, edit this section in place. Git keeps the history.

# Design

## Usage

*Write this first. Two or three real call sites in the caller's code: what they import, what they call, what comes back. The types below are derived from this. When they disagree, fix the types.*

## Types

*One code block with every new or changed type and public signature, bodies omitted. Data structures first. A reader traces input to output from this block alone.*

## Contract

*Vocabulary table, function entries, invariants table, and one ordered flow per use case in Usage, per [`design-contract.md`](design-contract.md). This part is the source of truth. Types, tickets, and code derive from it.*

## Decision

*At most five lines. The base candidate and why. Each graft and its source. Each rejected shape and the one reason it lost. The tradeoffs accepted, as "we accept X for Y". Name anything a reader might mistake for an oversight.*

## Open questions

*Only questions that change the contract. Phrase each as a question. Write "None" when there are none.*
