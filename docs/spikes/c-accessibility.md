# Spike C: accessibility — needs a person at the machine

**Status: not run.** Pass criterion (plan §5): VoiceOver reads continuously inside the paginated iframe across at least 3 page boundaries, and we record whether its reading position is observable so the visible page can follow it (X3, T2). NVDA is deferred with Windows.

## Why it is not automated

VoiceOver's reading position is not exposed to web content, and judging continuous reading needs a person listening. About 10 minutes with VoiceOver (⌘F5) on the harness page is needed. The fallback, if continuous reading fails, is in the plan: Pages mode offers “Read from here”, which switches to Scroll for the session.
