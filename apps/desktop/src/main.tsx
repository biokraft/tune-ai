import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { pickClient } from "./client/pickClient";
import { createStudioStore } from "./store/studioStore";
import "./styles/app.css";

const store = createStudioStore(pickClient());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App store={store} />
  </React.StrictMode>,
);
