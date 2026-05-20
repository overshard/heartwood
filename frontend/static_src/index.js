import "@fontsource/monaspace-argon";
import "./styles/base.scss";

document.querySelectorAll(".clone-box input").forEach((el) => {
  el.addEventListener("focus", () => el.select());
  el.addEventListener("click", () => el.select());
});
