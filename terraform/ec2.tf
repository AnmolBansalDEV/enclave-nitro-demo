data "aws_ami" "amazon_linux_2023" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-2023*-x86_64"]
  }
}
resource "aws_iam_instance_profile" "ecr_access" {
  name = "ecr-access-profile"

  role = aws_iam_role.ecr_access.name
}

resource "aws_iam_role" "ecr_access" {
  name = "ecr-access-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })
}

resource "aws_iam_policy_attachment" "ecr_access" {
  name       = "ecr-access-policy-attachment"
  roles      = [aws_iam_role.ecr_access.name]
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
}

resource "aws_instance" "enclave_instance" {
  ami           = data.aws_ami.amazon_linux_2023.id
  instance_type = "c5.xlarge"
  subnet_id     = module.vpc.public_subnets[0]
  iam_instance_profile = aws_iam_instance_profile.ecr_access.name


  vpc_security_group_ids = [
    aws_security_group.instance_sg.id
  ]

  user_data = base64encode(templatefile("${path.module}/user-data.sh.tpl", {
    eifArtifactPath = var.eif_artifact_path
  }))

  enclave_options {
    enabled = true
  }

  tags = {
    Name = var.project_name
  }

  lifecycle {
    ignore_changes = [
      ami
    ]
  }
}
