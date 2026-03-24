# QA with agents is broken

AI agents are writing code. They're deploying services. They're managing infrastructure. But when it comes to testing — actually verifying that software works — we're still stuck in the old world.

## The problem

Testing today requires code. You write assertions in the same language as the system under test. You maintain test fixtures, mock servers, factory functions. The test suite becomes its own codebase — with its own bugs, its own tech debt, its own maintenance burden.

Now add an AI agent to this picture. The agent can write test code, sure. But it's writing code to test code. It's operating at the wrong level of abstraction. The agent doesn't need to express "status code should be 200" in Python — it can just look at the response and see that it's 200.

The deeper problem: agents can't use existing test frameworks effectively because those frameworks assume a human developer who understands the codebase internals. They assume you know which function to call, which mock to set up, which database state to seed. An agent working from the outside — the way agents increasingly operate — has none of this context.

## Why it's hard

Three things make agent-driven QA genuinely difficult:

**Interpretation.** A human tester reads a spec and understands intent. "The login endpoint should return a token" — a human knows to check the response body for a JSON field. Traditional test frameworks can't interpret. They can only execute hardcoded assertions.

**Adaptation.** When a test fails, a human investigates. They poke around, try different inputs, read error messages. Traditional tests are static — they run the same steps every time regardless of what they observe.

**Judgment.** Some behaviors are fuzzy. "The response should be fast" or "the error message should be helpful." Humans make judgment calls. Traditional tests need exact thresholds.

Agents have all three capabilities. But we don't give them tools that use these capabilities. We give them pytest and tell them to write assertions.

## A different approach

What if the test isn't code at all?

Write a markdown file describing what the software should do. No syntax, no imports, no assertions. Just natural language:

```markdown
## POST /login
- Accepts email and password
- Returns 200 with token on valid credentials
- Returns 401 on bad credentials
```

Give an agent two things: this spec and a way to poke the running system. The agent reads the spec, sends requests, observes responses, and judges whether the behavior matches. The spec is the doc is the test.

This collapses the gap between "what the software should do" and "how we verify it." The spec is human-readable. The verification is agent-driven. There's no test code to maintain.

## What changes

When the test is a document instead of code:

- Anyone can write a spec. Product managers, support engineers, security reviewers. Not just developers.
- The spec doesn't break when the implementation changes. "Returns 200 with token" is true regardless of which framework serves it.
- The agent adapts. If the endpoint moves from `/login` to `/auth/login`, a hardcoded test breaks. An agent reading "the login endpoint" might try both.
- Coverage becomes a directory listing. Add a file, you added a test.

The trade-off is determinism. Two runs might produce different verdicts because the agent's interpretation varies. This is real, and it matters for CI. But it's the same trade-off humans make — two QA engineers might disagree on whether a behavior matches a spec. The question is whether the value of natural language specs and adaptive testing outweighs the cost of occasional non-determinism.

For many teams, it does.
