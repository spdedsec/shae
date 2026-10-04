with open("ROADMAP.md", "r") as f:
    lines = f.readlines()

for i, line in enumerate(lines):
    if "Aliases" in line:
        lines[i] = "- [x] Aliases `and`, `or`, `not` for `&&`, `||`, `!`.\n"
        break

with open("ROADMAP.md", "w") as f:
    f.writelines(lines)
