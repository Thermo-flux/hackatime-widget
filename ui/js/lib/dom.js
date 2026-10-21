export const $ = (id) => document.getElementById(id);

/** Build an element with text set via textContent, never innerHTML. */
export function el(tag, { class: cls, text } = {}, ...children) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  node.append(...children);
  return node;
}
