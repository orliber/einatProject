# הגדרות GitHub שאור מפעיל ידנית

אי אפשר להגדיר את הדברים האלה מהקוד. הם חלק מבקרות שרשרת האספקה (`STANDARDS.md` §5.8).

1. **Branch protection ל-`main`:** Require a pull request, 1 approval, Require review from Code Owners, Require status checks (`Rust`, `Supply chain`, `UI`, ו-`Desktop build` בשלוש מערכות ההפעלה), Require signed commits, Do not allow force pushes or deletions.
2. **Code security:** להפעיל Secret scanning + Push protection, Dependabot alerts ו-Dependabot security updates.
3. **Actions:** Settings → Actions → General: "Allow only actions created by GitHub and select non-GitHub actions", ולאשר רק: `pnpm/action-setup`, `Swatinem/rust-cache`. בנוסף: Workflow permissions = Read.
4. **נעילת Actions לפי SHA:** המשימה פתוחה. מסביבת הפיתוח לא הייתה גישה ל-API של GitHub כדי לשלוף SHA. אפשר להריץ `pinact` או לאשר את ה-PRs של Dependabot, שיחליף תגיות ב-SHA.
5. **הריפו נשאר פרטי.**
