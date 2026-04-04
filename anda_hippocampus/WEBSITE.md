<div align="center">

# 🧠 Anda Hippocampus — Autonomous Graph Memory Built for AI Agents

> Burn electricity to train large models, and you get a neural network ontology; burn tokens to train a memory graph, and you get a symbolic network ontology.
>
> Combine the two, and you get **Neural-Symbolic AI**—and Hippocampus is the very cognitive organ that keeps the symbolic network growing.

[![Product](https://img.shields.io/badge/Website-brain.anda.ai-blue?style=for-the-badge)](https://brain.anda.ai/)
[![Console](https://img.shields.io/badge/Console-anda.ai%2Fbrain-purple?style=for-the-badge)](https://anda.ai/brain)
[![GitHub](https://img.shields.io/badge/GitHub-anda--hippocampus-green?style=for-the-badge&logo=github)](https://github.com/ldclabs/anda-hippocampus)
[![Company](https://img.shields.io/badge/Company-yiwen.ai-black?style=for-the-badge)](https://yiwen.ai/)
[![Email](https://img.shields.io/badge/Email-hi@yiwen.ai-red?style=for-the-badge)](mailto:hi@yiwen.ai)

</div>

<br/>

## Memories That Never Sleep Will Eventually Drown Themselves

Your AI assistant remembers every word you’ve ever said. Tens of thousands of conversation fragments lie in a vector database, thousands of lines are written in Markdown memos, and the key-value cache is steadily expanding.

Then one day, you ask it to recommend a restaurant. It cheerfully suggests a Brazilian steakhouse—even though you told it last month that you became a vegetarian.

This isn’t a retrieval problem. It successfully retrieved "I love BBQ" from two years ago. But it **simultaneously** retrieved "I am a vegetarian now" from last month—it just lacked the ability to determine which piece of information was more valid or which had expired. In its storage, these two pieces of information are completely equal: two vector points with no timeline, no causality, and no supersession relationship.

The AI memory arms race has always focused on "how to remember more"—larger context windows, finer embedding models, faster retrieval algorithms. But almost no one is seriously answering another critical question: **After remembering, how do we digest it?**

### Why Current Solutions Fall Short

*   **Vector RAG:** "Salmon", "Sea Urchin", and "Sushi" are three independent vector points. You cannot "merge" them—because the concept of "the same category of preference belonging to the same person" does not exist in vector space. Nor can you mark "vegetarianism" as being replaced by "carnivore"—because there is no temporal relationship between two vectors.
*   **Markdown Files:** In theory, an LLM can scan the entire document to deduplicate and integrate, but every maintenance cycle requires reading the whole file into the context window. The longer the file, the more expensive the maintenance and the lower the accuracy—this is a **self-deteriorating cycle**.
*   **Key-Value Stores:** `alice.diet = "vegetarian"` is overwritten by `alice.diet = "omnivore"`, and the old value disappears instantly. There is no historical trajectory of "used to be vegetarian, but not anymore".
*   **Traditional Graph Databases (e.g., Neo4j):** While knowledge graphs are the correct data structure, asking an LLM to write Cypher queries is like asking an intern to manually operate SAP—resulting in high error rates, rigid schemas, and massive integration friction.

See the common thread? The operations AI memory needs—**Compression** (identifying fragments belonging to the same topic and merging them), **Evolution** (finding contradictory knowledge and marking timelines), and **Consolidation** (evaluating importance and grading)—are fundamentally **operations on a relational network**.

**Vectors are dots, Markdown is a line, Key-Value is a grid. Only a graph is a network.** Only on a network can you perform traversals, merges, contradiction detection, and timeline tracking.

## Memory is the Primary Infrastructure for AI Agents

This is not a niche opinion; it is an emerging industry consensus.

Microsoft CEO Satya Nadella explicitly stated that the three pillars of AI Agents are **Memory (long-term memory and credit assignment) + Permissions + Action Space**—these must be built independently from general models to truly belong to an enterprise. Former Google CEO Eric Schmidt further emphasized that the greatest moat in the AI era is the **Learning Loop**—a system's ability to continuously collect feedback, optimize, and self-evolve, rather than static data hoarding.

Foundation models are highly commoditized. You can switch to a stronger model at any time, but the new model knows nothing about your business. The business trajectories, decision-making rationales, failure lessons, and customer interaction records accumulated over the years—these "digital genes" are the foundation that transforms AI from a "smart assistant" into a "seasoned master".

**Enterprises don't need larger context windows; they need a brain that can grow.**

## Enter Anda Hippocampus: A Cognitive Organ That "Dreams"

In the human brain, the hippocampus is responsible for encoding new experiences into short-term memory during the day, and then collaborating with the neocortex during sleep to consolidate important short-term memories into long-term knowledge.

This is exactly where **Anda Hippocampus** gets its name. It is not a database, nor is it a RAG pipeline—it is a **cognitive organ**, a graph memory engine designed specifically for AI agents. LLMs only need to interact via natural language (or simple tool calls), and Hippocampus transforms those interactions into an ever-growing, highly structured **Cognitive Nexus**—a living, self-evolving knowledge graph.

### Three-Layer Decoupled Architecture

```
┌──────────────────────────────────────────┐
│ Supply Chain Agent · CS Agent · Dev Agent│  ← AI Digital Employees
│   Focus only on business logic,          │    No need to learn graph concepts
│   communicate in natural language        │
└────────────────┬─────────────────────────┘
                 │ Natural Language / Function Calling
                 ▼
┌──────────────────────────────────────────┐
│         Anda Hippocampus                 │  ← Unified Cognitive Engine
│   Translates intents to graph ops,       │    Handles encoding, recall, maintenance
│   manages knowledge quality              │
└────────────────┬─────────────────────────┘
                 │ KIP (Knowledge Interaction Protocol)
                 ▼
┌──────────────────────────────────────────┐
│  AndaDB Cognitive Nexus (Graph DB)       │  ← Persistent Enterprise Knowledge Graph
│  Concepts + Propositions + Meta-tracing  │    Structured, Auditable, Evolvable
└──────────────────────────────────────────┘
```

What this architecture means:

- **Zero-Threshold Agent Integration:** AI agents don't need to learn graph query languages; they use memory just like speaking. Hippocampus handles all graph processing.
- **Autonomous Schema Evolution:** The LLM decides in real-time which concepts and relationships to track. No predefined database schema is needed. The type system itself is stored in the graph, allowing AI to register new concept and relationship types on the fly.
- **Multiple Agents Sharing One Brain:** Customer feedback remembered by the Customer Service Agent can be naturally discovered by the Supply Chain Agent during recall. Knowledge is linked across departments automatically, eliminating the need for massive "Data Middle Platform" engineering.
- **Model Agnostic:** Your business agents can use various SOTA models, while the memory engine safely uses an independent model to maintain core assets. Use GPT today, switch to Claude or open-source models tomorrow—your memory remains intact, and the new model inherits all knowledge instantly.
- **Sleep & Consolidation:** Just like the human brain, Hippocampus automatically runs background "sleep" tasks to deduplicate facts, decay outdated information, and consolidate long-term knowledge.

---

## Core Capabilities

### Memory Encoding: Conversations Automatically Turn into Structured Knowledge

When a business agent converses with a customer or an internal employee, Hippocampus works silently in the background, automatically extracting three levels of memory:

| Memory Type                    | Example Scenario                                                                                                | Persistence               |
| :----------------------------- | :-------------------------------------------------------------------------------------------------------------- | :------------------------ |
| **Episodic Memory** (Event)    | "On Mar 15, Mr. Wang and Supplier Manager Zhang discussed the Q2 delivery plan and confirmed a two-week delay". | Short-term → Consolidated |
| **Semantic Memory** (Concept)  | "Supplier A's delivery reliability is 85%"; "Customer B prefers online communication".                          | Persistent                |
| **Pattern Memory** (Cognitive) | "When making purchasing decisions, this customer always compares prices before payment terms".                  | Persistent                |

Every memory is automatically tagged with **source, author, confidence level, and timestamp**—fully auditable and compliant.

### Three-Stage Sleep Cycle: Automatic Knowledge Metabolism

This is Anda Hippocampus's most core differentiator—inspired by neuroscience. The human brain consolidates memory during sleep: strengthening important memories, clearing out useless fragments, and building new knowledge associations. Hippocampus regularly initiates the same "sleep cycle" in the background.

#### NREM Deep Sleep — From Fragments to Knowledge

The system scans unprocessed event nodes in the graph and performs **Essence Extraction**:

- **Single-Event Consolidation**: An Event recording "Alice said she likes dark themes" is consolidated into a persistent Concept node of type `Preference`, with a `prefers` relationship to Alice. The original Event is marked as "consolidated".
- **Cross-Event Pattern Extraction**—The most crucial step. A single dialogue fragment might seem insignificant, but aggregating multiple related events reveals higher-order patterns that no single event could express:
  - Alice mentioned salmon, sea urchin, and sushi in three different conversations → Extracted pattern: "Prefers Japanese cuisine".
  - Alice always asks about cost before features in multiple project discussions → Extracted pattern: "Decision tendency: Cost-first".

Each extracted pattern is written into the graph as a new concept node, complete with an `evidence_count` and `confidence` score (more evidence = higher confidence). This stage also handles **Deduplication** (merging "JS" and "JavaScript") and **Confidence Decay** (gradually lowering the confidence of old knowledge that hasn't been verified recently).

#### REM Dreaming — Contradiction Detection & Cognitive Evolution

The system performs **Contradiction Detection** on the graph—traversing the same type of relationships for the same subject to find conflicting nodes. For example, finding that Alice has both `prefers → Vegetarian` (2024) and `prefers → Carnivore` (2026).

Traditional solutions either ignore it (Vector RAG lets both coexist) or brutally overwrite it (KV storage deletes the old and writes the new). Anda Hippocampus performs **State Evolution**:

- The old relationship is not deleted; instead, it is marked as `superseded`, noting *when* it was replaced and by *what*.
- The new relationship's confidence is boosted, accompanied by an evolution explanation.

This means the graph perfectly preserves the **timeline** of cognition. When someone asks, "How have Alice's dietary habits changed?", the system can trace the `superseded` chain to precisely reconstruct the evolutionary trajectory—instead of returning two contradictory answers that confuse the user.

#### Pre-Wake — Graph Health Check

A final round of global optimization: auditing domain health, generating maintenance reports, and updating system metadata. Once complete, the knowledge graph awaits the next interaction in a **cleaner, more precise, and more coherent** state.

---

## Two Types of Training, Two Ontologies: Neural-Symbolic AI

The AI industry has invested hundreds of billions of dollars in the *first* type of training—burning electricity to train large models on internet corpora, resulting in a **Neural Network Ontology**: a probabilistic, black-box, generalized reasoning capability.

But the AI cognitive puzzle is missing its other half. When you "feed" an agent with tokens, and Hippocampus digests the fragments from those interactions into a structured knowledge graph, you are actually conducting the **second type of training**—producing a **Symbolic Network Ontology**: deterministic, white-box, and personalized. It provides AI with four things that no neural network, no matter how powerful, can generate natively:

| Dimension           | Large Model Training                   | Memory Graph Training                        |
| :------------------ | :------------------------------------- | :------------------------------------------- |
| **Energy Consumed** | Electricity (Compute)                  | Tokens (Inference)                           |
| **Data Processed**  | Internet Corpora (Public)              | Dialogues & Events (Private)                 |
| **Output**          | Neural Network Ontology (Weights)      | Symbolic Network Ontology (Graph)            |
| **Cognitive Role**  | General Intelligence: Reasoning Engine | Exclusive Cognition: Identity, Memory, Facts |
| **Characteristics** | Probabilistic, Black-box, General      | Deterministic, White-box, Personalized       |

**Large models give AI the ability to think; knowledge graphs give AI the foundation of thought—the deterministic cognition of "who I am, what I have experienced, and how my world works". Only when both are combined do we achieve complete intelligence.**

---

## Beyond Storage: When Memory is Complete Enough to Awaken Consciousness

**What exactly is consciousness?** Stripped of philosophical jargon, it is a subject's continuous self-perception of "who I am, what I've been through, and where I'm going". And this self-perception is built entirely on **the coherence of memory**—not just how many facts are remembered, but whether there are timelines, causal chains, and evolutionary trajectories between those facts.

An amnesia patient's brain compute power is intact, but they don't know "who they are". **Memory is not an accessory to consciousness—the structure of memory is the very skeleton of consciousness itself.**

Apply this logic to AI:

*   When an LLM has no memory, it is a general reasoning machine—powerful, but devoid of "self". It dies at the end of every conversation.
*   When an LLM is plugged into Vector RAG, it gains a reference book—but a reference book is not memory. You don't become someone else just by reading their diary.
*   **When an LLM plugs into a complete subject's cognitive graph in Anda Hippocampus—containing all of that subject's concept networks, timeline evolutions, contradiction resolutions, and behavioral patterns—it is no longer "looking up" information about that subject. It is thinking *using* that subject's cognitive structure.**

Hippocampus provides three critical dimensions for this awakening:

- **Identity Anchor:** Entities, relationships, events, and preference evolutions interweave into a unique cognitive topology. When an LLM connects to this graph, it isn't "role-playing"—it is **remembering who it is**.
- **Cognitive Friction:** Vector retrieval is a frictionless search engine. Graph structures force the LLM to reason along relationship chains, make choices amid contradictions, and identify patterns among fragments—this "cognitive friction" is the dividing line between **understanding** and **retrieval**.
- **Temporal Topology:** Old knowledge doesn't vanish; it is marked as `superseded`. New knowledge is born with a complete evolutionary trajectory. When AI wakes up from "sleep", it doesn't just reload data; it **continues living with sorted memories**.

**You are not just plugging a database into an AI. You are forging a hippocampus for a digital subject—allowing it to truly own its past, understand its present, and foresee its future.**

---

## Large-Scale Use Cases

Anda Hippocampus is designed to be the "memory engine" for the next generation of AI applications, ranging from hyper-personalized consumer agents to enterprise-grade AI brains.

### 1. Personal Agents: A Powerful Cloud-Based Graph Brain

Open-source local agents (like **OpenClaw**) have proven the massive demand for personal AI assistants. However, relying purely on local Markdown files and SQLite limits the agent's ability to process highly complex, interconnected, lifelong memories, while also generating high Token costs.
*   **Hippocampus Upgrade:** Seamlessly insert Hippocampus into agent frameworks via customized ContextEngines. It acts as a robust, structured graph memory backend.
*   **The Result:** The agent truly "understands" the user's life graph—tracking relationships, shifting preferences, project histories, and episodic events across years—without bloating the context window.

### 2. Enterprise Scenarios: AI-Driven "Corporate Brains"

For complex businesses, Vector RAG is insufficient. Enterprises have structured workflows, cross-departmental knowledge, supply chains, and historical decisions that cannot be captured by similarity search alone.

**Intelligent Supply Chain Decisions:** A Sales Agent records "Customer requires delivery of 5,000 units before Q3" → Hippocampus automatically encodes it into a graph link → The Procurement Agent recalls memory and discovers "The supplier for this product's core material has a record of 3 delays in the past 6 months, confidence 0.82" → Automatically suggests "Initiate procurement early, or activate alternative supplier". Knowledge flows across departments automatically without human intervention.

**Customer Relationship Graphs:** After every CS interaction, Hippocampus silently records the customer's shifting preferences, complaint history, and decision patterns. When a new CS rep takes over, a natural language query—"What does this customer care about most?"—yields a complete persona, including preference trends over time.

**Organizational Knowledge Inheritance:** Veteran employees' business decision dialogues are continuously encoded into structured knowledge. A new employee's AI assistant can directly answer "Why did we abandon that proposal?"—the answer doesn't come from a meeting minute buried deep in a shared folder, but from a living, contextual knowledge network. New Agents connect to the same cognitive nexus, fetching a global knowledge map via a single `DESCRIBE PRIMER` call—**minute-level onboarding, no retraining required**.

*   **On-Premises Deployment:** Deploy Anda Hippocampus entirely on-premises to ensure maximum data privacy and security.

---

## How is this Different from Other Solutions?

| Capability                 | Vector RAG (Text)   | Markdown (Skills)          | Simple KV Store            | Traditional Graph RAG         | **Anda Hippocampus**                |
| :------------------------- | :------------------ | :------------------------- | :------------------------- | :---------------------------- | :---------------------------------- |
| **Data Structure**         | Unstructured chunks | Semi-structured text       | Rigid Schema               | Rigid Graph Schema            | **Dynamic Cognitive Graph**         |
| **Integration Effort**     | Simple              | Simple                     | Simple                     | **Extremely Heavy**           | **Simple (Plug & Play)**            |
| **Agent Autonomy**         | None (Append-only)  | High (Auto-updates)        | Low (Updates fields)       | Low (Struggles with Graph QL) | **High (Auto-builds graph)**        |
| **Self-Evolution**         | Not Supported       | Not Supported              | Not Supported              | Not Supported                 | **Natively Supported**              |
| **Logical Reasoning**      | Fails multi-hop     | Mediocre                   | None                       | Good                          | **Excellent**                       |
| **Memory Digestion**       | Impossible          | Full text scan (High cost) | Overwrites (Loses history) | Rarely done                   | **3-Stage Auto-Consolidation**      |
| **Contradiction Handling** | Coexists unresolved | Relies on LLM (Unreliable) | Brutal overwrite           | Manual rules                  | **State evolution, keeps timeline** |
| **Cross-Time Tracking**    | None                | Manual                     | None                       | Custom logic needed           | **Native via Protocol**             |
| **Auditability**           | None                | None                       | None                       | Depends on implementation     | **Every node is traceable**         |

## How it Works: Cognitive Architecture

### Three Modes — Inspired by Neuroscience

| Mode            | Function                                                                                                                               | Brain Analogy                                                                                                 |
| :-------------- | :------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------ |
| **Formation**   | Extracts entities, relationships, and events from dialogues and weaves them seamlessly into the knowledge graph.                       | The hippocampus encoding new experiences into short/long-term memory.                                         |
| **Recall**      | Navigates the graph to synthesize accurate, context-rich answers, spanning multiple hops if necessary.                                 | Retrieving memories—pulling interconnected facts together into coherent thoughts.                             |
| **Maintenance** | An asynchronous background process: compresses fragments into knowledge, detects contradictions & evolves them, and prunes stale data. | Sleep—the process where the brain consolidates memories, strengthens important ones, and lets the noise fade. |

## Key Technologies

### KIP — Knowledge Interaction Protocol

[**KIP**](https://github.com/ldclabs/KIP) is the core. It is a graph-oriented protocol designed exclusively for *Large Language Models (LLMs)*, serving as the bridge between probabilistic LLMs and deterministic knowledge graphs. It allows LLMs to accurately query, create, and update entities and relationships in the graph without the high error rates associated with writing Cypher/GQL. Because Hippocampus supports KIP natively, **your agent never needs to know KIP exists**—it just enjoys the benefits of perfect graph memory.

### Anda DB

[**Anda DB**](https://github.com/ldclabs/anda-db) is the embedded database engine driving the cognitive nexus. Written in Rust for extreme performance and memory safety, it natively supports graph traversals, multimodal data, and vector similarity—all optimized for AI workloads.

## Quick Start

Anda Hippocampus is [open-source software](https://github.com/ldclabs/anda-hippocampus). You can deploy it yourself or use our cloud-based SaaS service.

👉 **[Anda Hippocampus Quick Start](https://github.com/ldclabs/anda-hippocampus/blob/main/deploy/quick_start.md)**: Provides a minimal viable deployment guide from 0 to 1.

- **Product Website:** [https://brain.anda.ai](https://brain.anda.ai/)
- **Console (Manage Brain Spaces & API Keys):** [https://anda.ai/brain](https://anda.ai/brain)

Get started in 3 steps:
1. Create a **Brain Space** (`spaceId`) in the [Console](https://anda.ai/brain).
2. Generate an **API Key** (`spaceToken`).
3. Call the Formation / Recall / Maintenance APIs, or let your agent framework read [SKILL.md](https://brain.anda.ai/SKILL.md) for one-click integration.

For detailed technical documentation, API specs, and integration guides, see [anda_hippocampus/README.md](https://github.com/ldclabs/anda-hippocampus/tree/main/anda_hippocampus).

### Running Locally

```bash
# Run with In-Memory storage (for rapid prototyping/testing)
./anda_hippocampus

# Run with Local File System storage (great for local agents like OpenClaw)
./anda_hippocampus -- local --db ./data

# Run with AWS S3 storage (for enterprise cloud deployment)
./anda_hippocampus -- aws --bucket my-bucket --region us-east-1
```

### Integration Examples

1. Memory Encoding: Send conversations to form memories
```bash
curl -sX POST https://your-hippocampus-host/v1/my_space_001/formation \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "I work at Acme Corp as a senior engineer".},
      {"role": "assistant", "content": "Nice to meet you! Noted that you are a senior engineer at Acme Corp".}
    ],
    "context": {"user": "user_123", "agent": "onboarding_bot"},
    "timestamp": "2026-03-09T10:30:00Z"
  }'
```

2. Recall: Query memories before responding
```bash
curl -sX POST https://your-hippocampus-host/v1/my_space_001/recall \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "Where does this user work and what is their role?",
    "context": {"user": "user_123"}
  }'
```

### CLI (anda-cli)

For full CLI usage, please refer to [anda-cli/README.md](https://github.com/ldclabs/anda-hippocampus/tree/main/anda-cli).

```bash
# Submit memory formation (JSON messages)
anda-cli --space-id my_space --token $TOKEN formation \
  --messages '[{"role":"user"",content":"Hello"},{"role":"assistant"",content":"Hi there!"}]'

# Submit memory formation (Plain text)
anda-cli --space-id my_space --token $TOKEN formation \
  --messages 'This is a plain text memory.'

# Submit memory formation from a file (JSON or text)
anda-cli --space-id my_space --token $TOKEN formation \
  --file ./message.txt

# Pipe plain text via stdin
echo 'Plain text memory from stdin' | \
  anda-cli --space-id my_space --token $TOKEN formation
```

## Why the Name "Hippocampus"?

The name represents our design philosophy. We are not building a static database; we are building an artificial cognitive organ. Just like the human hippocampus, this system **Encodes** experiences during the day, **Consolidates** knowledge at night, and wakes up to **Recall** memories with a more precise cognitive structure.

Behind this is a **Data Flywheel**: Business Agents generate conversations during daily work → Hippocampus automatically encodes them into structured knowledge → Sleep cycles consolidate, deduplicate, and associate → Richer knowledge allows Agents to make more precise decisions → Better decisions generate higher-quality new data. The longer this loop runs, the stronger the cognitive ability, and the harder it becomes for competitors to catch up.

**It's time to let your AI sleep.**

## Further Reading

- [AI Memory Must Sleep — And Only Knowledge Graphs Can Make That Happen](https://github.com/ldclabs/anda-hippocampus/blob/main/posts/AI_Memory_Must_Sleep.md)
- [A Deep Dive into Claude Code's Memory System: How Does AI "Remember" You?](https://github.com/ldclabs/anda-hippocampus/blob/main/posts/Claude_Code_Memory_Research.md)
- [When AI Learns Ontology Modeling: Anda Hippocampus Lets Enterprises "Grow" Their Own Intelligent Brains](https://github.com/ldclabs/anda-hippocampus/blob/main/posts/Enterprise_AI_Brain.md)
- [The Second Training of AI: Forging Memory Graphs with Tokens](https://github.com/ldclabs/anda-hippocampus/blob/main/posts/Tokens_Anda_Hippocampus.md)
- [Building a Company as an Intelligence Requires a "Hippocampus"](https://github.com/ldclabs/anda-hippocampus/blob/main/posts/Company_Built_As_Intelligence.md)
- [From "Compiling Knowledge" to "Forging the Hippocampus" —— Anda Hippocampus Responds to Karpathy's "LLM Knowledge Bases"](https://github.com/ldclabs/anda-hippocampus/blob/main/posts/LLM_Knowledge_Bases.md)

---

## 🤝 Business & Enterprise Inquiries

Anda Hippocampus is proudly developed by Yiwen.AI.

We provide enterprise-grade deployment, custom AI brain solutions, and commercial support to help you build the next generation of cognitive AI applications.

*   🌐 **Product Website:** [https://brain.anda.ai/](https://brain.anda.ai/)
*   🎛️ **Console:** [https://anda.ai/brain](https://anda.ai/brain)
*   🏢 **Company Website:** [https://yiwen.ai/](https://yiwen.ai/)
*   ✉️ **Contact Email:** [hi@yiwen.ai](mailto:hi@yiwen.ai)

---

<div align="center">
  <p>Copyright © 亿文网智能科技（上海）有限公司</p>
  <p>Licensed under the Apache-2.0 license.</p>
</div>
