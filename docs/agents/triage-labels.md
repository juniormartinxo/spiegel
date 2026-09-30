# Triage Labels

The skills speak in terms of five canonical triage roles. The `SPG` project in It's a Plan has no dedicated triage labels, so each role maps to a column (by `stateType`), an assignee or delegate, or the `Planning` label.

| Role in mattpocock/skills | Representation in `SPG`                                          | Meaning                                  |
| ------------------------- | ---------------------------------------------------------------- | ---------------------------------------- |
| `needs-triage`            | `backlog` column, no assignee, no `Planning` label               | Maintainer needs to evaluate this issue  |
| `needs-info`              | label `Planning` (stays in `backlog`)                            | Waiting on reporter for more information |
| `ready-for-agent`         | `unstarted` column (Todo), delegate **Dusky Agent**              | Fully specified, ready for an AFK agent  |
| `ready-for-human`         | `unstarted` column (Todo), assignee **Junior Martins**           | Requires human implementation            |
| `wontfix`                 | `canceled` column                                                | Will not be actioned                     |

When a skill says "apply label X", apply the representation from this table with `update_issue` instead. Dusky Agent is an AI agent, so it goes in `delegateUserId`; people go in `assigneeUserId`. When moving out of `needs-info`, remove the `Planning` label. Resolve column, label and user ids via `get_project` (`projectKey: "SPG"`); never create new labels.

Edit the right-hand column to change the vocabulary.
