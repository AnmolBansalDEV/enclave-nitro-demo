variable "project_name" {
  type        = string
  description = "The project name that will be tagged in the resources"
}

variable "eif_artifact_path" {
  type        = string
  description = "The full OCI path of the EIF"
}

variable "deployment_id" {
  description = "Unique identifier for this deployment"
  type        = string
}