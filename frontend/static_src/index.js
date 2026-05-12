import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/600.css";
import "./styles/base.scss";

document.querySelectorAll(".clone-box input").forEach((el) => {
  el.addEventListener("focus", () => el.select());
  el.addEventListener("click", () => el.select());
});
