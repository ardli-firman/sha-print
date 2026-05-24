using System;
using System.Drawing;
using System.Windows.Forms;
using MaterialSkin;
using MaterialSkin.Controls;

namespace ShaPrint.App
{
    public class AboutForm : MaterialForm
    {
        public AboutForm()
        {
            this.Text = "About ShaPrint";
            this.Size = new Size(400, 320);
            this.StartPosition = FormStartPosition.CenterParent;
            this.MaximizeBox = false;
            this.MinimizeBox = false;

            var materialSkinManager = MaterialSkinManager.Instance;
            materialSkinManager.AddFormToManage(this);

            var layout = new TableLayoutPanel
            {
                Dock = DockStyle.Fill,
                RowCount = 5,
                ColumnCount = 1,
                Padding = new Padding(20, 20, 20, 10),
                BackColor = Color.Transparent
            };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));
            
            MaterialLabel lblName = new MaterialLabel
            {
                Text = "ShaPrint",
                FontType = MaterialSkinManager.fontType.H4,
                AutoSize = true,
                Anchor = AnchorStyles.None
            };
            layout.Controls.Add(lblName, 0, 0);

            MaterialLabel lblVersion = new MaterialLabel
            {
                Text = $"Version {System.Reflection.Assembly.GetExecutingAssembly().GetName().Version}",
                FontType = MaterialSkinManager.fontType.Subtitle1,
                AutoSize = true,
                Anchor = AnchorStyles.None,
                Margin = new Padding(0, 5, 0, 0)
            };
            layout.Controls.Add(lblVersion, 0, 1);

            MaterialLabel lblAuthor = new MaterialLabel
            {
                Text = "Author: ardli-firman",
                FontType = MaterialSkinManager.fontType.Body1,
                AutoSize = true,
                Anchor = AnchorStyles.None,
                Margin = new Padding(0, 10, 0, 0)
            };
            layout.Controls.Add(lblAuthor, 0, 2);

            MaterialLabel lblDesc = new MaterialLabel
            {
                Text = "A Virtual Printer and Print Server solution\nfor Windows networks without the hassle\nof SMB sharing.",
                FontType = MaterialSkinManager.fontType.Body2,
                AutoSize = true,
                TextAlign = ContentAlignment.MiddleCenter,
                Anchor = AnchorStyles.None,
                Margin = new Padding(0, 15, 0, 20)
            };
            layout.Controls.Add(lblDesc, 0, 3);

            MaterialButton btnClose = new MaterialButton
            {
                Text = "Close",
                AutoSize = false,
                Size = new Size(100, 36),
                Anchor = AnchorStyles.None,
                Type = MaterialButton.MaterialButtonType.Contained,
                UseAccentColor = false
            };
            btnClose.Click += (s, e) => this.Close();
            layout.Controls.Add(btnClose, 0, 4);

            this.Controls.Add(layout);
        }
    }
}
