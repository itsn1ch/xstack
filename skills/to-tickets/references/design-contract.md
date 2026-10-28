# Design contract

The contract is the single source of truth for a design. The to-tickets skill writes it, implementation honors it, and tickets mode slices it. It lives in the `Contract` part of the epic's Design section (`# Design` in `spec.md`, or the design comment on a remote spec issue). On any conflict between a ticket, a comment, or a chat message and the contract, the contract wins. Change the contract first, then everything derived from it.

The contract has three parts, in this order. Each part may only use names defined in the part before it.

## 1. Vocabulary

One table. Every domain concept the design names gets exactly one row. Use the project's domain glossary terms when they exist. One concept, one name. A synonym anywhere else (code, tickets, prose) is a defect.

| Name | Kind | Meaning | Source of truth | Writer |
|---|---|---|---|---|
| `Order` | entity | A customer's request to buy, from cart checkout until fulfilled or cancelled. | `orders/model.ts` `Order` | `OrderService` |
| `Money` | value | An amount in minor units plus an ISO currency code. | `shared/money.ts` `Money` | none (immutable) |
| `OrderStatus` | enum | `pending`, `paid`, `shipped`, `cancelled`. | `orders/model.ts` `OrderStatus` | `OrderService` |

Kinds are a closed set:

- **entity.** Has an identity that outlives changes to its fields, and a lifecycle. Two entities with equal fields are still different entities.
- **value.** Immutable, compared by content, no identity. Replaced, never mutated.
- **enum.** A closed set of named cases. Adding a case is a contract change.
- **event.** An immutable fact that something happened, named in past tense (`OrderPaid`).

Rules:

- **Source of truth** names the one type definition that defines the concept. Every other representation (wire DTO, DB row, cache entry) is derived from it at a boundary and is not listed as a second source.
- **Writer** names the one module allowed to create or change it. An entity with two writers is the split-ownership red flag. Fix the design, not the table.
- A name is final once the contract is agreed. Renaming is a contract change that updates the table, the sketch, and every open ticket in the same change.

## 2. Functions

One entry per public function or method in the sketch. Internal helpers are not listed. Use this shape.

```
### `payOrder(orderId: OrderId, payment: Payment): Result<Order, PayError>`

- **Intent.** Why the function exists, in one sentence from the caller's view. "Turn a pending order into a paid one exactly once, so checkout can charge without double-billing."
- **Owner.** The module that holds it. Must be the Writer of every entity it changes.
- **Reads.** Vocabulary names it reads.
- **Writes.** Vocabulary names it creates or changes. Empty means pure.
- **Preconditions.** What must be true before the call, each as a checkable predicate. "`order.status == pending`."
- **Postconditions.** What is true after a successful call. "`order.status == paid` and exactly one `OrderPaid` event exists for `orderId`."
- **Invariants.** Invariant IDs from part 2b that this function must preserve or establish.
- **Errors.** Each failure case by name, and the state left behind. "`PayError.AlreadyPaid`: no state change."
- **Idempotency.** What happens on a second identical call and on a crash halfway, per **make-operations-idempotent**.
```

Intent rules. The intent says what the caller gets and why, never how the body works. If two functions have the same intent, one of them is the two-ways-to-do-one-task red flag. If you cannot write the intent in one sentence, the function does too much. Split it.

### 2b. Invariants

One table. An invariant is a statement about the vocabulary that is true at every point a caller can observe.

| ID | Statement | Scope | Enforced by | Broken if | Proof |
|---|---|---|---|---|---|
| `INV-1` | An `Order` has at most one `OrderPaid` event. | `Order` | unique key on `(orderId)` in the payments table, and `payOrder` precondition | `payOrder` runs twice concurrently without the key | test: two concurrent `payOrder` calls, one succeeds, one returns `AlreadyPaid` |
| `INV-2` | `Order.total` equals the sum of its line `Money` amounts. | `Order` | type: `total` is derived, not stored | anyone stores `total` as a field | type check: no setter exists |

Rules:

- **Statement** is a predicate someone could evaluate against real state. "Orders are consistent" is not an invariant.
- **Scope** is one or more vocabulary names. Every entity in the vocabulary has at least one invariant or a stated reason it has none.
- **Enforced by** is the strongest available mechanism, in order: type, boundary validation, runtime check, test, prose. Name the weakest acceptable one and why the stronger ones do not fit, per **encode-lessons-in-structure**.
- **Broken if** names the realistic way an agent would break it. This is the line a future contributor reads.
- **Proof** names the test or check that fails when the invariant breaks. An invariant without a proof is a guess.

## 3. Flow

The ordered sequence from the caller's entry point to the final observable result. One numbered list per use case in the Design section's Usage, in the same order as that usage.

```
Use case: customer pays for an order

1. `checkout(cartId)` (external input: `cartId`) → produces `Order` (status `pending`)
2. `quotePayment(Order)` consumes `Order` from 1 → produces `PaymentQuote`
3. `payOrder(Order.id, Payment)` consumes `Order` from 1, `Payment` from external input, guided by `PaymentQuote` from 2 → produces `Order` (status `paid`), `OrderPaid`
4. `shipOrder(Order.id)` consumes `OrderPaid` from 3 → produces `Order` (status `shipped`)
```

Rules:

- **Every input has an earlier source.** Each consumed name must come from an earlier step or be marked external input. A step that consumes something a later step produces is out of order. Reorder the flow or fix the design. Never leave it.
- **Numbering is execution order.** No step is listed before a step it depends on. If two steps are independent, mark them `2a` and `2b` so the parallelism is explicit, not accidental.
- **Every function in part 2 appears in some flow.** A function in no flow is dead or the flow is incomplete.
- **Every state transition of an `enum` lifecycle appears in order.** For `OrderStatus`, the flows together must walk `pending → paid → shipped`, and show where `cancelled` can enter.
- **The flow matches the Usage.** If the caller's usage calls B before C, the flow lists B before C. When they disagree, fix the flow or the usage, then the sketch.

## Contract checks

Run these before the contract is agreed and again before tickets mode publishes. Any failure blocks the hand-off.

1. Every name used in parts 2 and 3 is a row in part 1, spelled the same.
2. Every entity has exactly one Writer, and every function's Writes are owned by its Owner.
3. Every invariant has a predicate Statement, an Enforced by, and a Proof.
4. Every function has a one-sentence Intent, and no two intents match.
5. Every flow step consumes only names produced earlier or marked external input.
6. Every public function appears in at least one flow.
