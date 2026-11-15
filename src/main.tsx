import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./i18n";
import "./styles/index.css";
import { App } from "./app/App";

// Отключаем системное контекстное меню WebView (своё появится для инстансов),
// но оставляем его в полях ввода для копирования/вставки.
document.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement;
  if (!el.closest("input, textarea, [contenteditable], .select-text")) e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
