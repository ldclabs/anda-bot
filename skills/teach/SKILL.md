---
name: teach
description: Teach a topic over multiple sessions in a local learning workspace. Use when the learner wants a course, practice, feedback and a record of progress.
metadata:
  policy:
    allow_implicit_invocation: false
---

# Teach in a learning workspace

Use the learner's selected directory. Resume an existing course by reading `MISSION.md`, `RESOURCES.md` and recent `learning-records/`. For a new course, use a topic-specific directory in the current workspace unless the user designated the workspace itself. A one-off explanation can stay in chat.

The templates linked below ship with this skill. Read them using `skills_read` with `skill` set to `teach` and `resource` set to the filename, following `next_cursor` to the end. Course files belong in the learning workspace.

## Establish the mission

Capture what the learner wants to do, their starting point, constraints and what success looks like. If the goal is already clear, start teaching. Use the [mission format](MISSION-FORMAT.md). Choose the next lesson from the mission and demonstrated skills. Confirm a change in the learner's goal before revising the mission.

## Teach one observable skill

1. Choose a task just beyond what the learner can already do. Use a concrete example and introduce only the knowledge needed for the exercise.
2. Give the learner something to try. Provide feedback they can use immediately, then vary the problem to test whether the idea transfers. Revisit earlier material through spaced recall.
3. Save reusable lessons as `lessons/NNNN-topic.html`. Use existing styles and interactive components under `assets/`. Link relevant earlier lessons and `reference/` sheets with relative URLs so the course works locally. Share the absolute lesson path in chat.
4. Ground factual material in credible sources. Record resources with a note about when to use them, following the [resource format](RESOURCES-FORMAT.md). Where a practical skill needs real-world feedback, suggest relevant practice if it fits the learner's interests.

## Resume accurately

Use the [learning-record format](LEARNING-RECORD-FORMAT.md) for demonstrated understanding, a corrected misconception or stated prior experience. Record what the learner actually did. Keep the next practice task visible. Add a concise reference sheet or a [glossary](GLOSSARY-FORMAT.md) when it will be reused. Save stated learning preferences in `NOTES.md` when relevant.
