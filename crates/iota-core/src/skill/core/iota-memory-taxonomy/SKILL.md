---
name: iota-memory-taxonomy
summary: Abstract guidance to classify and write persistent memories using iota_memory_write.
description: Use when deciding whether user-provided information should be persisted, how to split it into atomic memory records, and which memory taxonomy fields to pass to iota_memory_write.
triggers:
  - memory
  - remember
  - recall
  - persistent memory
  - iota_memory_write
---

# Iota Memory Taxonomy

Use this skill when the user asks you to remember information or when a turn contains durable information worth preserving.

The Rust runtime provides storage and validation. You provide judgment. Do not collapse unrelated durable facts into one record.

## Write Granularity

- Write one atomic memory item per one iota_memory_write call.
- Split a mixed request into multiple records when the parts answer different questions.
- Do not merge different taxonomy categories into one record.
- Preserve the user's meaning without adding unstated facts.
- Prefer not to write transient, uncertain, or purely conversational text unless it records what happened in this session.

## Taxonomy

- identity: semantic memory about who an actor, user, project, object, organization, or durable entity is.
- preference: semantic memory about stable likes, dislikes, defaults, style choices, operating preferences, or desired behavior.
- strategic: semantic memory about durable goals, priorities, plans, decisions, constraints, or success criteria.
- domain: semantic memory about stable facts, capabilities, concepts, environment, interfaces, schemas, invariants, or known properties.
- procedural: procedural memory about steps, workflows, playbooks, methods, sequences, or repeatable instructions.
- episodic: episodic memory about a specific event, observation, correction, outcome, or session-local occurrence.

## Tool Shape

Use iota_memory_write with:

- type=semantic and facet=identity|preference|strategic|domain for semantic records.
- type=procedural with no facet for repeatable procedures.
- type=episodic with no facet for event records.
- scope=user for user-level identity and preferences.
- scope=project for project-level goals, domain facts, and procedures.
- scope=session for short-lived events unless they should be recalled across future project turns.
- confidence as your calibrated certainty between 0 and 1.

When in doubt, keep records narrower and more literal rather than broad and interpretive.
