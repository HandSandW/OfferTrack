import type { PathState } from "../../contracts";
export const pathStateText: Record<PathState, string> = {
  available: "目录可访问（不代表附件未被占用）",
  missing: "目录不存在，可能已被移动或改名；不会自动重建",
  wrongType: "原目录位置被文件等其他项目占用；不会覆盖",
  busy: "目录正在被其他程序占用，请关闭相关程序后重试",
  accessDenied: "没有访问权限，不能据此判断附件是否丢失",
  unsafe: "路径不安全或包含重解析点，已拒绝访问",
  unavailable: "暂时无法检查目录，请检查磁盘状态后重试",
};
