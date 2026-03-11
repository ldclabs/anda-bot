# 🧠 Anda Hippocampus (海马体) — Long-Term Memory for AI Agents

> Give AI a brain that remembers.

## The Problem: AI Has No Memory

Today's AI agents are extraordinarily intelligent — they can reason, write code, analyze data, and carry on sophisticated conversations. But they share a fundamental weakness: **they forget everything the moment a conversation ends**.

Every time you start a new session with an AI assistant, you're talking to someone with complete amnesia. Your preferences, your history, your relationships, your past decisions — all gone. You have to re-explain who you are, what you're working on, and what you need, over and over again.

This is like visiting a doctor who erases their memory after every appointment. No matter how brilliant they are, they can never build on what they've learned about you.

## The Solution: A Hippocampus for AI

In the human brain, the **hippocampus** is the region responsible for forming and retrieving long-term memories. It converts your daily experiences into lasting knowledge — transforming "what just happened" into "what I know".

**Anda Hippocampus** does exactly this for AI agents. It is a dedicated memory service that gives any AI agent the ability to:

- **Remember** — conversations, facts, preferences, relationships, and events persist across sessions
- **Recall** — retrieve relevant knowledge naturally, just like asking someone "what do you remember about X?"
- **Maintain** — automatically organize, consolidate, and prune memory, just as your brain does during sleep

## How Is This Different from Other Memory Systems?

Most "AI memory" solutions fall into two categories — neither truly solves the problem:

### 1. Raw Text Storage (e.g., vector databases, chat log retrieval)

These systems save chunks of conversation text and use similarity search to retrieve them later. While simple, they have fundamental limitations:

- **No understanding** — they store raw text blobs without extracting meaning
- **No structure** — facts, events, preferences, and relationships are all mixed together
- **No evolution** — memories never consolidate, organize, or decay; the database grows without bound
- **Fragile retrieval** — slight changes in wording can miss relevant memories
- **No deduplication** — the same fact stored 10 times results in noise, not clarity

### 2. Simple Key-Value Storage (e.g., user profile fields)

These systems store structured data like `{"name": "Alice", "language": "Chinese"}`. They're organized but rigid:

- **Fixed schema** — can only remember what was pre-defined
- **No relationships** — cannot represent "Alice works with Bob on Project Aurora"
- **No context** — cannot capture *why* a preference exists or *when* a decision was made
- **No nuance** — no confidence levels, no source tracking, no temporal awareness

### 3. Anda Hippocampus: Knowledge Graph + LLM Intelligence

Anda Hippocampus takes a fundamentally different approach, inspired by how the human brain actually works:

| Capability              | Raw Text          | Key-Value    | Anda Hippocampus                |
| ----------------------- | ----------------- | ------------ | ------------------------------- |
| Remembers facts         | Implicitly        | Explicitly   | Structured knowledge nodes      |
| Remembers relationships | No                | No           | Directed graph links            |
| Remembers events        | Appends text      | No           | Episodic memory with timestamps |
| Detects patterns        | No                | No           | Cognitive memory extraction     |
| Self-organizes          | No                | No           | Sleep-mode maintenance          |
| Deduplicates            | No                | No           | Automated merging               |
| Decays stale knowledge  | No                | No           | Confidence scores with decay    |
| Schema evolution        | N/A               | Rigid schema | Dynamic, self-describing schema |
| Query interface         | Similarity search | Key lookup   | Natural language                |

The key insight: **memory isn't just storage — it's an active, living system**. Your brain doesn't simply record events; it extracts meaning, builds connections, consolidates knowledge during sleep, and gradually forgets what's no longer relevant. Anda Hippocampus brings this same lifecycle to AI.

## How It Works

An AI agent using Anda Hippocampus doesn't need to understand any of the underlying complexity. It simply talks to the Hippocampus in natural language:

```
┌─────────────────────┐
│   Business Agent    │  ← Your AI agent. Knows nothing about knowledge graphs.
│  (No special setup) │     Just speaks natural language.
└────────┬────────────┘
         │ Natural Language
         ▼
┌─────────────────────┐
│    Hippocampus      │  ← The memory layer. Understands how to read and write
│   (LLM + KIP)      │     knowledge graphs using KIP protocol.
└────────┬────────────┘
         │ KIP (Knowledge Interaction Protocol)
         ▼
┌─────────────────────┐
│  Cognitive Nexus    │  ← The knowledge graph database. Facts, relationships,
│  (Knowledge Graph)  │     events, patterns — all structured and searchable.
└─────────────────────┘
```

### Three Modes — Inspired by the Human Brain

| Mode            | What It Does                                                                                   | Brain Analogy                                                                                         |
| --------------- | ---------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Formation**   | After a conversation, extracts and stores knowledge: facts, preferences, relationships, events | The hippocampus encoding new experiences into memory                                                  |
| **Recall**      | Given a question, searches and synthesizes an answer from stored knowledge                     | Retrieving a memory — pulling together related facts to answer a question                             |
| **Maintenance** | Periodically consolidates, deduplicates, and prunes the knowledge graph                        | Sleep — when your brain organizes memories, strengthens important ones, and lets irrelevant ones fade |

### A Concrete Example

Suppose your AI customer service agent has a conversation with Alice:

> **Alice:** "I just moved from Beijing to Shanghai. My new address is 123 Nanjing Road. And please switch my account to dark mode — I always prefer dark themes."

**Formation** will extract and store:
- **Fact:** Alice's current city is Shanghai (updating previous fact: Beijing)
- **Event:** Alice moved from Beijing to Shanghai
- **Fact:** Alice's address is 123 Nanjing Road
- **Preference:** Alice prefers dark mode
- **Pattern:** Alice consistently prefers dark UI themes

Two weeks later, a different agent needs to answer:

> "What do we know about Alice's preferences?"

**Recall** will search the knowledge graph and synthesize:

> "Alice prefers dark mode for interfaces. She recently relocated from Beijing to Shanghai."

A month later, **Maintenance** runs and:
- Consolidates the "moved from Beijing" event into a stable fact (Alice lives in Shanghai)
- Slightly decays the confidence of the address (since addresses can change)
- Keeps the dark mode preference at high confidence (it's a stable pattern)

## Key Technologies

### KIP — Knowledge Interaction Protocol

[**KIP**](https://github.com/ldclabs/KIP) is the protocol that bridges LLMs and knowledge graphs. It provides memory and cognitive operation primitives designed for AI agents — think of it as SQL for knowledge graphs, but designed to work naturally with LLMs.

The beauty of Anda Hippocampus is that **your agent never needs to know KIP exists**. The Hippocampus handles all KIP operations internally.

### Anda DB

[**Anda DB**](https://github.com/ldclabs/anda-db) is the embedded database engine that powers the Cognitive Nexus. Written in Rust for performance and reliability, it supports multi-modal data, full-text search, and vector similarity search — all optimized for AI workloads.

## Quick Start

For detailed technical documentation, see [anda_hippocampus/README.md](./anda_hippocampus/README.md).

```bash
# Run with in-memory storage (for development/testing)
./anda_hippocampus

# Run with local filesystem storage
./anda_hippocampus -- local --db ./data

# Run with AWS S3 storage
./anda_hippocampus -- aws --bucket my-bucket --region us-east-1
```

## Why "Hippocampus"?

The name is not a metaphor — it's a design philosophy. Just as the human hippocampus:

1. **Encodes** daily experiences into memory (**Formation** mode)
2. **Retrieves** relevant memories when needed (**Recall** mode)
3. **Consolidates** memories during sleep, strengthening important ones and letting trivial ones fade (**Maintenance** mode)

Anda Hippocampus implements this same cognitive architecture for AI agents, creating a memory system that is alive, structured, and self-maintaining.

## License

Copyright © LDC Labs

Licensed under the MIT or Apache-2.0 license.
