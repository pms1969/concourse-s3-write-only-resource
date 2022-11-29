#![deny(clippy::all)]
#![deny(clippy::nursery)]

use concourse_resource::*;
use concourse_s3_no_check_resource::S3WriteOnly;

create_resource!(S3WriteOnly);
