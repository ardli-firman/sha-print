---
name: review-single-domain-per-agent
description: "Limit reviewer agents to one functional domain to prevent excessive analysis time"
condition: "DiscoveryBypassTests.*ImageProcessorTests"
scope: "tool:task"
---

Each reviewer agent MUST cover exactly one functional domain. Grouping tests from different domains (e.g. network discovery + image processing) causes the agent to context-switch, re-read unrelated infrastructure, and extend runtime unpredictably. Split across agents: one for DiscoveryBypassTests, another for ImageProcessorTests. If domain boundaries are unclear, err on the side of splitting.