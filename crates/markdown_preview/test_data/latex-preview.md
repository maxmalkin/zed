# Package-aware math preview

Default packages: $\mathbb{R}$ and $\frac{1}{2}$.

$$
\begin{aligned}
a &= b+c \\
d &= \begin{pmatrix}1 & 2 \\ 3 & 4\end{pmatrix}
\end{aligned}
$$

To test the included local package, point `latex.package_directory` at this
folder and add `\usepackage{previewlocal}` to `latex.preamble` in settings:

$$
\text{\previewlocalmessage}
$$

Code remains literal: `$not_math$`.
