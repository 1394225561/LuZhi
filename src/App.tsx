import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function App() {
  const [greetMsg, setGreetMsg] = useState("");
  const [name, setName] = useState("");

  async function greet() {
    // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
    setGreetMsg(await invoke("greet", { name }));
  }

  return (
    <main className="flex min-h-screen items-center justify-center bg-[#040506] bg-[radial-gradient(84.6%_73.49%_at_50%_26.51%,rgba(4,63,150,0.28),rgba(6,18,37,0.08))] p-8 text-white">
      <section className="w-full max-w-xl rounded-2xl border border-white/6 bg-[#07080a] p-8 text-left shadow-[rgba(255,255,255,0.05)_0_1px_0_0_inset,rgba(0,0,0,0.4)_0_4px_40px_8px] max-[520px]:p-6">
        <p className="mb-2 text-xs font-medium tracking-[0.04em] text-[#9c9c9d]">
          录屏工作台
        </p>
        <h1 className="m-0 text-[32px] leading-[1.2] tracking-[-0.06px] text-white">
          录智
        </h1>
        <p className="mb-6 mt-2 text-[#9c9c9d]">录屏、美化、导出</p>

        <form
          className="flex gap-3 max-[520px]:flex-col"
          onSubmit={(e) => {
            e.preventDefault();
            greet();
          }}
        >
          <input
            id="greet-input"
            className="min-w-0 flex-1 rounded-lg border border-white/6 bg-white/5 px-3 py-2 text-sm font-medium text-white outline-none"
            onChange={(e) => setName(e.currentTarget.value)}
            placeholder="输入测试名称"
          />
          <button
            className="rounded-lg border border-white/6 bg-[#e6e6e6] px-3 py-2 text-sm font-medium text-[#2f3031] shadow-[rgba(0,0,0,0.4)_0_1.5px_0.5px_2.5px,rgb(0,0,0)_0_0_0.5px_1px,rgba(0,0,0,0.25)_0_2px_1px_1px_inset,rgba(255,255,255,0.2)_0_1px_1px_1px_inset] hover:border-white/25 active:bg-[#d8d8d8]"
            type="submit"
          >
            发送问候
          </button>
        </form>
        <p className="mt-4 min-h-6 text-[#9c9c9d]">{greetMsg}</p>
      </section>
    </main>
  );
}

export default App;
