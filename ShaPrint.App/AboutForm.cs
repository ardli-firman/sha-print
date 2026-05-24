using System;
using System.Drawing;
using System.Windows.Forms;

namespace ShaPrint.App
{
    public class AboutForm : Form
    {
        public AboutForm()
        {
            this.Text = "About ShaPrint";
            this.Size = new Size(350, 250);
            this.StartPosition = FormStartPosition.CenterParent;
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;
            this.MinimizeBox = false;

            Label lblName = new Label
            {
                Text = "ShaPrint",
                Font = new Font("Segoe UI", 16, FontStyle.Bold),
                AutoSize = true,
                Location = new Point(120, 20)
            };
            this.Controls.Add(lblName);

            Label lblVersion = new Label
            {
                Text = $"Version {Application.ProductVersion}",
                AutoSize = true,
                Location = new Point(130, 55)
            };
            this.Controls.Add(lblVersion);

            Label lblAuthor = new Label
            {
                Text = "Author: ardli-firman",
                AutoSize = true,
                Location = new Point(120, 80)
            };
            this.Controls.Add(lblAuthor);

            Label lblDesc = new Label
            {
                Text = "A Virtual Printer and Print Server solution\nfor Windows networks without the hassle\nof SMB sharing.",
                TextAlign = ContentAlignment.MiddleCenter,
                AutoSize = true,
                Location = new Point(45, 110)
            };
            this.Controls.Add(lblDesc);

            Button btnClose = new Button
            {
                Text = "Close",
                Location = new Point(125, 160),
                Size = new Size(80, 30)
            };
            btnClose.Click += (s, e) => this.Close();
            this.Controls.Add(btnClose);
        }
    }
}
