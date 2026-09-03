import { open } from "@tauri-apps/plugin-dialog";

export async function selectFullBackup(): Promise<string | null> {
  const selection = await open({
    multiple: false,
    directory: false,
    title: "选择 OfferTrack 完整备份",
    filters: [
      { name: "OfferTrack 完整备份", extensions: ["offertrack-backup"] },
    ],
  });
  return typeof selection === "string" ? selection : null;
}

export async function selectDirectory(title: string): Promise<string | null> {
  const selection = await open({
    directory: true,
    multiple: false,
    title,
  });

  return typeof selection === "string" ? selection : null;
}
