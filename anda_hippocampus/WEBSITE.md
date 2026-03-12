<div align="center">

# 🧠 Anda Hippocampus (海马体)
### Autonomous Graph Memory for AI Agents

*Give AI a self-evolving cognitive brain.*

[![Product](https://img.shields.io/badge/Website-brain.anda.ai-blue?style=for-the-badge)](https://brain.anda.ai/)
[![GitHub](https://img.shields.io/badge/GitHub-anda--hippocampus-green?style=for-the-badge&logo=github)](https://github.com/ldclabs/anda-hippocampus)
[![Company](https://img.shields.io/badge/Company-yiwen.ai-black?style=for-the-badge)](https://yiwen.ai/)
[![Email](https://img.shields.io/badge/Email-hi@yiwen.ai-red?style=for-the-badge)](mailto:hi@yiwen.ai)

</div>

<br/>

## The Memory Bottleneck: From "Storage" to "True Cognition"

We are no longer in the era where "AI has no memory." Today's AI agents use long context windows, vector databases (RAG), simple Key-Value stores, and Markdown files (like Agent Skills) to remember user interactions.

However, they suffer from a fundamental **Cognitive Bottleneck**:
*   **Vector RAG is just a text pile:** It retrieves messy, isolated chunks of text based on similarity. It cannot connect the dots or perform multi-hop logical reasoning.
*   **Markdown storage is a manual struggle:** While many modern agents rely on updating Markdown files to store core "Skills" or "Memories," this approach is hard to scale. As the files grow, LLMs struggle to maintain consistency, avoid duplicates, and retrieve relevant context without blowing up the token window.
*   **Existing Graph solutions are too heavy:** While Knowledge Graphs are the obvious answer to complex reasoning, integrating traditional graph databases (like Neo4j) with AI agents is notoriously difficult. Forcing LLMs to write complex graph query languages (like Cypher) leads to high error rates, rigid schemas, and massive integration friction.

**Memory is not just a hard drive; it is a living, breathing network of connections.** When a human remembers, they don't search a text log; they traverse a neural graph of entities, relationships, events, and temporal changes.

## Enter Anda Hippocampus: LLM-Autonomously Built Graph Memory

**Anda Hippocampus** is a revolutionary memory service that empowers LLMs to **autonomously construct and evolve a dynamic Knowledge Graph**.

Instead of forcing developers to build rigid schemas or heavy database integrations, Hippocampus handles the complexity under the hood. The LLM simply interacts in natural language (or via simple tool calls), and Hippocampus translates this into an ever-growing, highly structured **Cognitive Nexus**.

With continuous use, the LLM organically builds a knowledge graph whose complexity and interconnectedness rival human neural networks.

### 🌟 Why Hippocampus is a Game-Changer:
- **Zero-Friction Integration:** Your AI agent doesn't need to learn graph query languages. It interacts naturally, and Hippocampus does the graph lifting.
- **Autonomous Schema Evolution:** The LLM decides what concepts and relationships to track on the fly. No pre-defined database schemas are required.
- **Neural-Level Cognition:** It connects isolated facts into a holistic world model, enabling true multi-hop reasoning (e.g., *“How does Alice's new job affect the project she started last year?”*).
- **Sleep & Consolidation:** Just like the human brain, Hippocampus automatically runs background "sleep" tasks to deduplicate facts, decay stale information, and consolidate long-term knowledge.

---

## 🚀 Massive Scale Use Cases

Anda Hippocampus is designed to be the "Memory Engine" for the next generation of AI applications, from hyper-personalized consumer agents to enterprise-grade AI brains.

### 1. The Personal Agent: A Powerful Cloud Brain for Frameworks like OpenClaw
Open-source local agents (like **OpenClaw**) have proven the massive demand for personal AI assistants. However, relying purely on local Markdown files and SQLite limits an agent's ability to handle highly complex, interconnected, and lifelong memories without blowing up token costs.
*   **The Hippocampus Upgrade:** Seamlessly plug Hippocampus into Agent frameworks via customized ContextEngines. It acts as a powerful, structured Graph Memory backend.
*   **The Result:** The agent truly "understands" the user's life graph—tracking relationships, changing preferences, project histories, and episodic events across years—without context window bloat. It provides a cloud-ready (or locally robust) cognitive brain for your personal digital twin.

### 2. The Enterprise Scenario: The AI-Driven "Enterprise Brain"
Vector RAG is not enough for complex businesses. Enterprises have structured workflows, tribal knowledge, supply chains, and historical decisions that cannot be captured by similarity search alone.
*   **Private Deployment:** Deploy Anda Hippocampus completely on-premise to ensure maximum data privacy and security.
*   **The Result:** Transform static enterprise wikis and disjointed databases into a **living Enterprise Brain**. AI agents can use this graph to perform complex decision support, automate intricate business workflows, onboard new employees instantly, and even **predict business trends** by analyzing the interconnected graph of past projects and market events.

---

## How Is This Different from the Rest?

| Capability             | Vector RAG (Text)    | Markdown (Skills)    | Simple Key-Value     | Traditional Graph RAG         | **Anda Hippocampus**           |
| :--------------------- | :------------------- | :------------------- | :------------------- | :---------------------------- | :----------------------------- |
| **Data Structure**     | Unstructured blobs   | Semi-structured text | Rigid schema         | Rigid graph schema            | **Dynamic Cognitive Graph**    |
| **Integration Effort** | Easy                 | Easy                 | Easy                 | **Extremely Heavy**           | **Easy (Plug & Play)**         |
| **Agent Autonomy**     | None (Just appends)  | High (Self-updates)  | Low (Updates fields) | Low (Struggles with Graph QL) | **High (Builds graph itself)** |
| **Logical Reasoning**  | Fails at multi-hop   | Moderate             | None                 | Good                          | **Exceptional**                |
| **Self-Maintenance**   | No (Database bloats) | Manual/LLM-Intensive | No                   | Rarely                        | **Yes (Sleep/Consolidate)**    |

## How It Works: The Cognitive Architecture

An AI agent using Anda Hippocampus doesn't need to understand any of the underlying graph complexity.

```text
┌─────────────────────┐
│   Your AI Agent     │  ← e.g., OpenClaw, Enterprise Assistant
│  (No graph setup)   │    Thinks and acts in Natural Language.
└────────┬────────────┘
         │ Natural Language / Function Calling
         ▼
┌─────────────────────┐
│    Hippocampus      │  ← The Cognitive Engine. Translates intent into graph
│    (LLM + KIP)      │    operations autonomously.
└────────┬────────────┘
         │ KIP (Knowledge Interaction Protocol)
         ▼
┌─────────────────────┐
│  Cognitive Nexus    │  ← The underlying Graph Database (Anda DB).
│  (Knowledge Graph)  │    Stores concepts, propositions, and episodic events.
└─────────────────────┘
```

### Three Modes — Inspired by Neuroscience

| Mode            | What It Does                                                                                                         | Brain Analogy                                                                                |
| :-------------- | :------------------------------------------------------------------------------------------------------------------- | :------------------------------------------------------------------------------------------- |
| **Formation**   | Extracts entities, relationships, and events from conversations and seamlessly weaves them into the Knowledge Graph. | The hippocampus encoding new experiences into short-term/long-term memory.                   |
| **Recall**      | Navigates the graph to synthesize exact, context-rich answers, traversing multiple links if necessary.               | Retrieving a memory—pulling together interconnected facts to form a coherent thought.        |
| **Maintenance** | An asynchronous background process that merges duplicates, adjusts confidence scores, and prunes obsolete data.      | Sleep—when the brain consolidates memories, strengthens the vital ones, and lets noise fade. |

## Key Technologies

### KIP — Knowledge Interaction Protocol
[**KIP**](https://github.com/ldclabs/KIP) is the secret sauce. It is a graph-oriented protocol designed *specifically for Large Language Models*. It acts as the bridge between probabilistic LLMs and deterministic Knowledge Graphs. Because Hippocampus natively speaks KIP, **your agent never needs to know KIP exists**—it just enjoys the benefits of perfect graph memory.

### Anda DB
[**Anda DB**](https://github.com/ldclabs/anda-db) is the embedded database engine that powers the Cognitive Nexus. Written in Rust for extreme performance and memory safety, it natively supports graph traversal, multi-modal data, and vector similarity—all optimized for AI workloads.

## Quick Start

For detailed technical documentation, API specs, and integration guides, see [anda_hippocampus/README.md](https://github.com/ldclabs/anda-hippocampus/tree/main/anda_hippocampus).

```bash
# Run with in-memory storage (for fast prototyping/testing)
./anda_hippocampus

# Run with local filesystem storage (Ideal for local Agents like OpenClaw)
./anda_hippocampus -- local --db ./data

# Run with AWS S3 storage (For Enterprise Cloud deployment)
./anda_hippocampus -- aws --bucket my-bucket --region us-east-1
```

## Why the name "Hippocampus (海马体)"?

The name is our design philosophy. We are not building a static database; we are building an artificial cognitive organ. Just like the human hippocampus, this system **Encodes** experiences, **Retrieves** complex narratives, and **Consolidates** knowledge during "sleep".

Anda Hippocampus transitions AI from merely "processing chat logs" to possessing a living, structured, and self-maintaining mind.

---

## 🤝 Business & Enterprise Inquiries

Anda Hippocampus is proudly developed by Yiwen.AI.

We provide enterprise-grade deployment, custom AI brain solutions, and commercial support to help you build the next generation of cognitive AI applications.

*   🌐 **Product Website:** [https://brain.anda.ai/](https://brain.anda.ai/)
*   🏢 **Company Website:** [https://yiwen.ai/](https://yiwen.ai/)
*   ✉️ **Contact Email:** [hi@yiwen.ai](mailto:hi@yiwen.ai)

---

<div align="center">
  <p>Copyright © 亿文网智能科技（上海）有限公司</p>
  <p>Licensed under the Apache-2.0 license.</p>
</div>
