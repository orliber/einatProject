// D-160: a file that takes long to read goes on in the background; one that is quick does not.
// Invented names only.
import { ipc, type ImportPreview } from "../../ipc/client";
import { BACKGROUND_AFTER_MS, isImageFile, readOrBackground, resetReads } from "./backgroundReads";

afterEach(() => {
  resetReads();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const preview = { title: "בדוי" } as ImportPreview;

describe("reading in the background", () => {
  it("photos and scanned pages go there at once, documents are read in front of her", () => {
    expect(["דף.png", "דף.JPG", "דף.jpeg", "דף.tif", "דף.tiff"].every(isImageFile)).toBe(true);
    expect(["דוח.pdf", "דוח.docx", "png.txt"].some(isImageFile)).toBe(false);
  });

  it("a quick file is answered as always", async () => {
    vi.spyOn(ipc, "importDocument").mockResolvedValue(preview);
    await expect(readOrBackground("c", new File(["x"], "דוח.pdf"))).resolves.toEqual({ preview });
  });

  it("a file still being read after a few seconds goes on in the background", async () => {
    vi.useFakeTimers();
    vi.spyOn(ipc, "importDocument").mockReturnValue(new Promise(() => undefined));
    const r = readOrBackground("c", new File(["x"], "סריקה.pdf"));
    await vi.advanceTimersByTimeAsync(BACKGROUND_AFTER_MS + 10);
    await expect(r).resolves.toBe("background");
  });

  it("a quick file that fails says so where she is", async () => {
    vi.spyOn(ipc, "importDocument").mockRejectedValue({ message: "מוצפן" });
    await expect(readOrBackground("c", new File(["x"], "דוח.pdf"))).rejects.toEqual({ message: "מוצפן" });
  });
});
