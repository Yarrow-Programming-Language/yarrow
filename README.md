<p align="center">
  <img src="assets/logo.jpg" alt="Yarrow" width="480">
</p>

<p align="center">
  <b>A typed, stack-based language with ownership, regions, and explicit unsafe</b>
</p>

---

This is a side-quest to rewrite this language in C without the use or with a minimal use of LLMs.

# Goals

- I want a standard language design independent from the compiler (like C standard but without the stdlib part)
- I want a compiler that transform it's ast to [QBE](https://c9x.me/compile/)
- I want an interpreter that read and interpret the ast

Just three gaols but a lot of work.
