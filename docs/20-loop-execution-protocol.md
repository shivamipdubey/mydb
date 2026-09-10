# Loop Execution Protocol

This is how you work through a phase from start to finish. Follow this loop for every phase in docs/03-phases-roadmap.md.

## The loop
1. Pick the next task from the current phase, using docs/24-task-breakdown-template.md to break the phase into ordered tasks if that has not been done yet.
2. Implement the task.
3. Run docs/16-security-and-cybersafety-checklist.md against the change.
4. Run the tests required by docs/18-testing-strategy.md for the area touched.
5. Update documentation per docs/19-documentation-rules.md.
6. Check the task against the phase's requirement list: does every feature named for this phase in docs/01-prd.md and docs/03-phases-roadmap.md now exist and work, including every screen, button, and confirmation step it depends on?
7. If something is missing or broken, fix it now, in this same loop iteration, before moving to the next task. Do not carry a known-broken item forward silently.
8. If everything for this task passes, move to the next task and repeat from step 1.
9. When every task in the phase is done, run the full exit condition check in docs/25-exit-conditions-definition-of-done.md against the whole phase, not just the last task.
10. If the exit condition check passes, the phase is complete; move to the next phase in docs/03-phases-roadmap.md. If it fails on any point, fix that point and re-run the full exit condition check again. Repeat until it passes.

## Exit condition for the loop itself
The loop for a phase ends only when step 10's check passes completely. Do not exit early because most items pass, and do not skip re-checking after a fix; a fix to one item can break another, so re-run the whole check, not just the item you fixed.

## What this loop is not
It is not a one-time check at the very end of all six phases. It runs at the end of every phase, and step 3 through 5 run after every single task inside that phase too.
