// WARN: Remove at some point
#![allow(unused)]

use reqwest::{blocking::Client};
use reqwest::header::{CONNECTION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json;
use std::env;
use std::os::fd::RawFd;
use std::ffi::c_int;
use std::io::{self, Write, IsTerminal};
use std::mem::MaybeUninit;
use libc;

const APPLICATION_JSON: &str = "application/json";
const KEEP_ALIVE: &str = "keep-alive";

const API_SESSION: &str = "/sb-api/api/public/v1.0/auth/session";
const API_VOLUME_ID: &str = "/sb-api/api/public/v1.0/volume/id";
const API_EVO: &str = "/v1/evo";

fn main()  {
    match run() {
        Ok(_) => {
            std::process::exit(0);
        }
        Err(e) => {
            println!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut buffer = String::with_capacity(1024);
    // if args.len() < 2 {
    //     return Err(
    //         io::Error::new(io::ErrorKind::InvalidInput, "Missing argument")
    //     );
    // }

    let mut termios = Termios::new()?;
    termios.set_mode(TermMode::Canonical)?;

    print!("EVO Address: ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut buffer)?;
    let evo = buffer.trim().to_string();
    buffer.clear();

    print!("Username: ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut buffer)?;
    let username = buffer.trim().to_string();
    buffer.clear();

    print!("Password: ");
    io::stdout().flush()?;
    termios.set_echo(Echo::Disable)?;
    io::stdin().read_line(&mut buffer)?;
    termios.set_echo(Echo::Enable)?;
    println!("\n");
    let password = buffer.trim().to_string();
    buffer.clear();

    let client = Client::new();
    let session = session(&client, evo, username, password)?;

    // println!("Session ID: {}", session.id);

    let url = format!("http://{}/{API_EVO}", &session.evo);
    let Some(evo_info) = client.get(url)
        .header(reqwest::header::ACCEPT, APPLICATION_JSON)
        .basic_auth(session.auth.name, Some(session.auth.password))
        .send().ok()
        .filter(|r| r.status() == 200)
        .and_then(|r| r.json::<EVO>().ok()) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Unable to retrieve EVO information"
            ))
    };

    println!("{} | v{}        Domain: {}", evo_info.hostname, evo_info.version, evo_info.domain);
    println!();

    loop {
        buffer.clear();

        print!("SBCLI: ");
        io::stdout().flush()?;

        io::stdin().read_line(&mut buffer)?;
        let input = buffer.trim();
        let (command, remainder) = parse_input(input);

        match command {
            Command::Logout => break,
            Command::Unknown => {
                println!("Unknown Command");
            }
        }
    }

    Ok(())
}

fn parse_input(input: &str) -> (Command, &str) {
    let bytes = input.as_bytes();
    let start = 0;
    let mut end = start;

    for byte in bytes {
        if byte.is_ascii_whitespace() {
            break;
        }

        end += 1;
    }

    let cmd = &input[start..end];

    let command = match cmd {
        s if s.eq_ignore_ascii_case("logout") => Command::Logout,
        _ => Command::Unknown,
    };

    (command, "")
}

#[derive(Deserialize, Debug)]
struct Timezone {
    name: String,
    offset: String,
}

#[derive(Deserialize, Debug)]
struct EVO {
  version: String,
  hostname: String,
  domain: String,
  uuid: String,
  slingshot: String,
  timezone: Timezone,
}

enum Command {
    Logout,
    Unknown,
}

enum ConnectionType {
    HTTP,
    HTTPS,
}

#[derive(Serialize)]
struct Auth {
    name: String,
    password: String,
}

struct Session {
    id: String,
    evo: String,
    auth: Auth,
}

fn session(
    client: &Client,
    evo: String,
    username: String,
    password: String,
) -> io::Result<Session> {
    let auth = Auth {
        name: username,
        password: password,
    };

    #[derive(Deserialize)]
    struct Data {
        cookie: u32,
    }

    #[derive(Deserialize)]
    struct Response {
        status: String,
        data: Option<Data>,
    }

    let url = format!("http://{evo}/{API_SESSION}");
    let auth_serialized = serde_json::to_string(&auth)?;

    let Some(cookie) = client
        .post(url)
        .header(CONTENT_TYPE, APPLICATION_JSON)
        .body(auth_serialized)
        .send().ok()
        .filter(|r| r.status() == 200)
        .and_then(|r| r.json::<Response>().ok())
        .filter(|r| r.status == "success")
        .and_then(|r| r.data)
        .map(|data| data.cookie.to_string()) else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unable to login"
            ));
        };

    Ok(Session {
        id: cookie,
        evo,
        auth,
    })
}

enum TermMode {
    Canonical,
    Noncanonical,
}

enum Echo {
    Enable,
    Disable,
}

struct Termios {
    fd: RawFd,
    original: libc::termios,
    current: libc::termios,
}

impl Termios {
    pub fn new() -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;
        let original = Termios::tcgetattr(fd)?;
        let current = original;

        Ok(Termios { fd, original, current })
    }

    fn tcgetattr(fd: RawFd) -> io::Result<libc::termios> {
        unsafe {
            let mut termios = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }

            return Ok(termios.assume_init());
        }
    }

    fn tcsetattr(fd: RawFd, termios: &libc::termios) -> io::Result<()> {
        unsafe {
            if libc::tcsetattr(fd, libc::TCSANOW, termios) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(())
    }

    pub fn set_mode(&mut self, mode: TermMode) -> io::Result<()> {
        match mode {
            TermMode::Canonical => {
                self.current.c_lflag |= libc::ICANON;
                self.current.c_lflag |= libc::ECHOE;
            }
            // WARN: I think this is a bug
            TermMode::Noncanonical => self.current.c_lflag |= libc::ICANON,
        }

        Self::tcsetattr(self.fd, &self.current)
    }

    pub fn set_echo(&mut self, status: Echo) -> io::Result<()> {
        match status {
            Echo::Disable => self.current.c_lflag &= !libc::ECHO,
            Echo::Enable => self.current.c_lflag |= libc::ECHO,
        }

        Self::tcsetattr(self.fd, &self.current)
    }

}

impl Drop for Termios {
    fn drop(&mut self) {
        unsafe {
            let _ = Self::tcsetattr(self.fd, &self.original);
        }
    }
}





// Disable the public API
// This request sets the permissions to none, disabling the public API.
// 
// cURL Example:
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" "http://127.0.0.1/sb-api/api/v1.0/preferencesservice/publicAPIMode" -X PUT -d 'none'

//  
// 
// Set API to Read-Only
// This request sets the permissions to Read-Only, the default permission scheme.
// 
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" "http://127.0.0.1/sb-api/api/v1.0/preferencesservice/publicAPIMode" -X PUT -d 'ro'
//  
// 
// Set API to Read-Write
// This is required to make any changes using the public API.
// 
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" "http://127.0.0.1/sb-api/api/v1.0/preferencesservice/publicAPIMode" -X PUT -d 'rw'
//  
// 
// Sessions
// Create a Session
// All API methods require a valid session. This method creates a session.
// 
// HTTP Request:
// POST http://evo-address/sb-api/api/public/v1.0/auth/session
// cURL Example:
// #To Create a session, use the POST method and include the login credentials.
// curl -H "Content-Type: application/json" -X POST -d '{"name":"q","password":"q"}' "http://127.0.0.1/sb-api/api/public/v1.0/auth/session"
// #Make sure to replace 127.0.0.1 with your EVO IP.
// This request body is in JSON format.
// 
// { 
//  "name":"q",
//  "password":"q"
// }
// 
// The above command returns JSON structured like this:
// 
// { 
//  "status":"success",
//  "data":{ 
//  "cookie":1447413700,
//  "user_id":2
//  }
// }
// Note: For external users, prepend domain and two backslashes. 
// 
// Example:
// 
// curl -H "Content-Type: application/json" -X POST -d '{"name":"SNS-AD\\editor1","password":"q"}' "http://192.168.1.25
// /sb-api/api/public/v1.0/auth/session"
// Using a Session_ID
// Every call to the API after the session is created expects a session cookie in the header. If there are no requests within an open session for 3 minutes, the session is closed.
// 
// cURL Example:
// curl -H "session_id: 1447413700"
//  
// 
// Delete a Session
// When finished making calls to the ShareBrowser API, it's best to delete your session to do this use the DELETE method.
// 
// HTTP Request:
// DELETE http://evo-address/sb-api/api/public/v1.0/auth/session
// cURL Example:
// # Make sure to specify your session and send a DELETE request.
// curl -H "session_id: 1447413700" -X DELETE "http://127.0.0.1/sb-api/api/public/v1.0/auth/session"
// #Make sure to replace 127.0.0.1 with your EVO IP and 1447413700 with your session_id.
//  
// 
// Volumes
// GET Volume ID
// Retrieves a Volume ID from ShareBrowser's database.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/volume/id?volume_uuid={volume_uuid}
// cURL Example:
// #Make sure to specify your session and send a GET request with the volume_uuid.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/volume/id?volume_uuid=09ABC165-86B1-46BB-A678-3E8A7D0882E4"
// The above command returns JSON structured like this:
// 
// {
//    "status":"success",
//    "data":{
//       "volume_id":2
//    }
// }
// Query Parameters:
//  volume_uuid 	
//  Can be found in the _ShareBrowserVolumeUID_ file on any NAS volume. 
// 
//  
// 
// Files
// GET File ID
// Retrieves a File ID from ShareBrowser's database.
// 
// HTTP Request
// GET http://evo-address/sb-api/api/public/v1.0/file/id?volume_id={volume_id}&path={path}
// cURL Example:
// # Make sure to specify your session and send a GET request with the volume_id and path.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/file/id?volume_id=2&path=%2Ffolderonroot%2Fmovie.mov"
// The above command returns JSON structured like this:
// 
// {
//    "status":"success",
//    "data":{
//       "file_id":23
//    }
// }
// Query Parameters:
//  volume_id 	 The volume_id returned from GET volume_id. 
// path	
//  The path to the file from the root of the volume to the filename separated by slashes (%2F) 
// 
//  
// 
// GET Metadata for a file
// Retrieves a file's metadata from ShareBrowser's database.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/file/metadata?file_id={file_id}
// cURL Example:
// # Make sure to specify your session and send a GET request with the file_id.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/file/metadata?file_id=23"
// The above command returns JSON structured like this:
// 
// {
//    "status":"success",
//    "data":{
//       "tags":"a,b,c",
//       "comment":"comment",
//       "harvestedMetadata":{
// 
//       }
//    }
// }
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// GET a filepath by file_id
// Retrieves a filepath for a file based on its ShareBrowser file_id.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/file/{file_id}/path
// cURL Example:
// # Make sure to specify your session and include the file_id in the URL.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/file/31/path"
// The above command returns JSON structured like this:
// 
// {
//   "status":"success",
//   "data":"/folder/img.jpg"
//   }
// Query Parameters:
// file_id	 The file_id returned from a previous request. 
//  
// 
// GET a full filepath with share name/id by file_id
// This endpoint is available in 7.1+.
// 
// Retrieves a filepath for a file based on its ShareBrowser file_id.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/file/{file_id}/full_path
// cURL Example:
// # Make sure to specify your session and include the file_id in the URL.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/file/{file_id}/full_path"
// The above command returns JSON structured like this:
// 
// {
// "status": "success",
// "data": {
// "file_path": "/M003C003_161207_R00H.mp4",
// "volume_id": 13,
// "volume_name": "Media"
// }
// }
// Query Parameters:
// file_id	 The file_id returned from a previous request. 
//  
// 
//  
// 
// Search Functions
// Searches can be completed by filename, tag, comment, or custom metadata.
// 
// Search by filename, tag, and comment
// Retrieves a list of files whose filenames, tags, or comments match a particular string.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/file/search?filename={filename}&tag={tag}&comment={comment}&pageno={pagenumber}&pagesize={pagesize}
// cURL Example:
// # Make sure to specify your session and include necessary data.
// curl -H "session_id: 1438904380" "http://127.0.0.1/sb-api/api/public/v1.0/file/search?filename=plane&pageno=0&pagesize=50"
// The above command returns JSON structured like this:
// 
// {
// 
//     "status": "success",
//     "data": {
//         "search_results": [
//             {
//                 "file_id": 1322,
//                 "file_path": "/plane.mp4",
//                 "volume_id": 13,
//                 "volume_name": "5151_qa",
//                 "volume_uuid": "32B4B8AA-D90D-4820-9E26-F608921EA3A1"
//             }
//         ],
//         "total_count": 1
//     }
// 
// }
// Query Parameters:
// filename	 The pattern to match in the filename. 
// tag	 The tag to match. 
// comment	 The comment string to match. 
// hmd (7.1+)	 A value to find within harvested metadata. 
// pageno	 The requested page number of results based on pagesize. 
// pagesize	 The requested page size for results. 
// volume_id (7.1+)	 A volume to search within 
// location (7.1+)	 A folder path to search within 
//  
// 
// Search Custom Metadata
// Get a list of files wherein the value contains a string for a specific custom metadata field (not tags/comments).
// 
// HTTP Request
// GET http://evo-address/sb-api/api/public/v1.0/file/search_custom_metadata?key={key}&value={value}&pageno={pagenumber}&pagesize={pagesize}
// cURL Example
// curl -H "session_id: 1438904380" "http://127.0.0.1/sb-api/api/public/v1.0/file/search_custom_metadata?key=TapeID&value=VTAPE1&pageno=0&pagesize=50"
// The above command returns JSON structured like this:
// 
// {
//     "status": "success",
//     "data": {
//         "search_results": [
//             {
//                 "file_id": 2538,
//                 "file_path": "/sdna/sdna-2/baking2.jpg",
//                 "volume_id": 1,
//                 "volume_name": "80_3_smb_qa"
//             },
//             {
//                 "file_id": 3068,
//                 "file_path": "/th/519.txt",
//                 "volume_id": 1,
//                 "volume_name": "80_3_smb_qa"
//             }
//         ],
//         "total_count": 2
//     }
// }
// Query Parameters:
// key	 The case-sensitive custom metadata field 
// value	 The search string 
// pageno	 The requested page number of results based on pagesize. 
// pagesize	 The requested page size for results. 
// volume_id (7.1+)	 A volume to search within 
// location (7.1+)	 A folder path to search within 
//  
// 
// Search Harvested Metadata (7.1+)
// Get a list of files wherein the value contains a string for a specific harvested metadata field.
// 
// HTTP Request
// GET http://evo-address/sb-api/api/public/v1.0/file/search_harvested_metadata?key={key}&value={value}&pageno={pagenumber}&pagesize={pagesize}
// cURL Example
// curl -H "session_id: 2088901711" -X GET "http://192.168.1.25/sb-api/api/public/v1.0/file/search_harvested_metadata?key=Codecs&value=h264"
// The above command returns JSON structured like this:
// 
// {
//     "status": "success",
//     "data": {
//         "search_results": [
//             {
//                 "file_id": 2538,
//                 "file_path": "/sdna/sdna-2/baking2.jpg",
//                 "volume_id": 1,
//                 "volume_name": "80_3_smb_qa"
//             },
//             {
//                 "file_id": 3068,
//                 "file_path": "/th/519.txt",
//                 "volume_id": 1,
//                 "volume_name": "80_3_smb_qa"
//             }
//         ],
//         "total_count": 2
//     }
// }
// Query Parameters:
// key	 The case-sensitive custom metadata field 
// value	 The search string 
// pageno	 The requested page number of results based on pagesize. 
// pagesize	 The requested page size for results. 
// volume_id	 A volume to search within 
// location	 A folder path to search within 
//  
// 
// Archive Requests
// Archive requests are available as of ShareBrowser 6.0
// 
// Get Archive Status
// Gets the archive status of a file.
// 
// HTTP Request
// GET http://evo-address/sb-api/api/public/v1.0/file/{file_id}/archived
// cURL Example
// curl --silent -H "session_id: 1447413700" GET "http://127.0.0.1/sb-api/api/public/v1.0/file/2506/archived"
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// Change Archive Status
// Sets the archive status of a file (true/false).
// 
// HTTP Request
// POST http://evo-address/sb-api/api/public/v1.0/file/{file_id}/archived?value={true|false}
// cURL Example
// curl --silent -H "session_id: 1447413700" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/file/2506/archived?value=true"
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// Custom Metadata
// Custom metadata requests are available as of ShareBrowser 6.0.
// 
// Get Custom Metadata
// Get the value of Custom Metadata for a file.
// 
// HTTP Request
// GET http://evo-address/sb-api/api/public/v1.0/file/{file_id}/custom_metadata?field_name={field_name}
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/file/26456/custom_metadata?field_name=Status"
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
// field_name	 (Optional) The field name to query. 
//  
// 
// Post Custom Metadata
// Set the value of a specific Custom Metadata field to text.
// 
// HTTP Request
// POST http://evo-address/sb-api/api/public/v1.0/file/{file_id}/custom_metadata?field_name={field_name}
// cURL Example
// curl -H "session_id: 1447413700" -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/file/26456/custom_metadata?field_name=Director" -d '"Spielberg"'
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
// field_name	 The field name to post. 
//  
// 
// Delete Custom Metadata
// Delete a specific custom metadata field/value from a file.
// 
// HTTP Request
// DELETE http://evo-address/sb-api/api/public/v1.0/file/{file_id}/custom_metadata?field_name={field_name}
// cURL Example
// curl -H "session_id: 1447413700" -H "Content-Type: application/json" -X DELETE "http://127.0.0.1/sb-api/api/public/v1.0/file/100/custom_metadata?field_name=status
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
// field_name	 The field name to delete. 
//  
// 
// Post a Custom List
// Post a new field with the value of an option in a list.
// 
// HTTP Request
// POST http://evo-address/sb-api/api/public/v1.0/file/{file_id}/custom_metadata?field_name={field_name}
// cURL Example
// curl -H "session_id: 1447413700" -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/file/26456/custom_metadata?field_name=Director" -d '["Spielberg"]'
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
// field_name	 The field name to post. 
//  
// 
// Post Tags
// Post ShareBrowser Tags to a file.
// 
// HTTP Request
// POST http://evo-address/sb-api/api/public/v1.0/file/{file_id}/tags
// cURL Example
// curl -H "session_id: 1447413700" -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/file/26456/tags" -d '["newtag"]'
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// Post Comment
// Post a ShareBrowser Comment to a file.
// 
// HTTP Request
// POST http://evo-address/sb-api/api/public/v1.0/file/{file_id}/comment
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/file/26456/comment" -d 'This is a comment'
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// Custom Metadata Template
// Manipulation of custom metadata template fields is possible as of ShareBrowser 6.0 and requires ShareBrowser Admin permissions.
// 
// Create Custom Text Field
// Create a metadata field to hold text (ShareBrowser 6.0). This requires ShareBrowser Admin Credentials.
// 
// HTTP Request
// PUT http://evo-address/sb-api/api/v1.0/customtemplate/field
// cURL Example
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" -X PUT "http://127.0.0.1/sb-api/api/v1.0/customtemplate/field" -d '{"name":"fieldname", "type":"text"}'
// Query Parameters:
// name	 The name for the field. 
// type	 The type of field text,list,protected, or invisible. 
//  
// 
// Create Custom List Field
// Create a metadata field to hold a value from a list (ShareBrowser 6.0). This requires ShareBrowser Admin Credentials.
// 
// HTTP Request
// PUT http://evo-address/sb-api/api/v1.0/customtemplate/field
// cURL Example
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" -X PUT "http://127.0.0.1/sb-api/api/v1.0/customtemplate/field" -d '{"name":"newfieldname", "type":"list", "options":["value1", "value2"], "multipleValues":true}'
// Query Parameters:
// name	 The name for the field. 
// type	 The type of field text,list,protected, or invisible. 
// options	 The options that may be selected from the list. 
// multipleValues	 Allow multiple values to be selected(true,false). 
//  
// 
// Create Invisible Text Field
// Create an invisible field to hold a value that the user cannot see (ShareBrowser 6.0). This requires ShareBrowser Admin Credentials.
// 
// HTTP Request
// PUT http://evo-address/sb-api/api/v1.0/customtemplate/field
// cURL Example
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" -X PUT "http://127.0.0.1/sb-api/api/v1.0/customtemplate/field" -d '{"name":"newfieldname", "type":"invisible"}'
// Query Parameters:
// name	 The name for the field. 
// type	 The type of field text,list,protected, or invisible. 
//  
// 
// Create Protected Text Field
// Create an protected field to hold a value that the user can see but not edit (ShareBrowser 6.0). This requires ShareBrowser Admin Credentials.
// 
// HTTP Request
// PUT http://evo-address/sb-api/api/v1.0/customtemplate/field
// cURL Example
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" -X PUT "http://127.0.0.1/sb-api/api/v1.0/customtemplate/field" -d '{"name":"newfieldname", "type":"protected"}'
// Query Parameters:
// name	 The name for the field. 
// type	 The type of field text,list,protected, or invisible. 
//  
// 
// Modify Custom List Field
// Modify a custom list field (ShareBrowser 6.0). This requires ShareBrowser Admin Credentials.
// 
// HTTP Request
// POST http://evo-address/sb-api/api/v1.0/customtemplate/field/{field_name}
// cURL Example
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/v1.0/customtemplate/field/nameoffield" -d '{"type":"list", "options":["value1", "value2"], "multipleValues":true}'
// Query Parameters:
// field_name	 The name for the field. 
// type	 The type of field text,list,protected, or invisible. 
// options	 The options that may be selected from the list. 
// multipleValues	 Allow multiple values to be selected(true,false). 
//  
// 
// Delete Custom Metadata Field
// Delete a custom field from the Custom Metadata Template. This requires ShareBrowser Admin Credentials.
// 
// HTTP Request
// DELETE http://evo-address/sb-api/api/v1.0/customtemplate/field/{field_name}
// cURL Example
// curl -u 'administrator:adminpw111' -H "Content-Type: application/json" -X DELETE "http://127.0.0.1/sb-api/api/v1.0/customtemplate/field/nameoffield"
// Query Parameters:
// field_name	 The name for the field. 
//  
// 
// Bins
// Working with ShareBrowser Bins via the API is available in ShareBrowser 6.0.
// 
// GET Bins
// Gets a list of public and private ShareBrowser Bins.
// 
// HTTP Request
// GET http://evo-address/sb-api/api/public/v1.0/bin
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/bin"
//  
// 
// Create a bin
// Creates a public or private ShareBrowser Bin.
// 
// HTTP Request
// PUT http://evo-address/sb-api/api/public/v1.0/bin
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X PUT "http://127.0.0.1/sb-api/api/public/v1.0/bin" -d '{"name":"new bin name", "isPrivate": false}'
//  
// 
// Add Files to a Bin
// Adds a list of file_ids to a specific bin.
// 
// HTTP Request
// POST "http://evo-address/sb-api/api/public/v1.0/bin/{bin_id}/files"
// cURL Example
// curl -H "session_id: 1447413700" -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/bin/13/files" -d '[26, 3214]'
// file_id	 The file_id returned from GET file_id. 
// bin_id	 The bin_id returned from GET Bins. 
//  
// 
// Update Bin
// Change a bin name or its private status.
// 
// HTTP Request
// POST "http://evo-address/sb-api/api/public/v1.0/bin/{bin_id}"
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X POST "http://127.0.0.1/sb-api/api/public/v1.0/bin/13" -d '{"name":"new bin name", "isPrivate": false}'
// bin_id	 The bin_id returned from GET Bins. 
//  
// 
// List Bin Contents
// Get a list of files within a bin.
// 
// HTTP Request
// GET "http://evo-address/sb-api/api/public/v1.0/bin/{bin_id}/files"
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/bin/13/files"
// bin_id	 The bin_id returned from GET Bins. 
//  
// 
// Remove Files from a Bin
// Removes a list of file_ids from a specific bin.
// 
// HTTP Request
// DELETE "http://evo-address/sb-api/api/public/v1.0/bin/{bin_id}/files"
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X DELETE "http://127.0.0.1/sb-api/api/public/v1.0/bin/13/files" -d '[97]'
// file_id	 The file_id returned from GET file_id. 
// bin_id	 The bin_id returned from GET Bins. 
//  
// 
// Delete Bin
// Removes a bin.
// 
// HTTP Request
// DELETE "http://evo-address/sb-api/api/public/v1.0/bin/{bin_id}"
// cURL Example
// curl --silent -H "session_id: 1447413700" -H "Content-Type: application/json" -X DELETE "http://127.0.0.1/sb-api/api/public/v1.0/bin/13"
// bin_id	 The bin_id returned from GET Bins. 
//  
// 
// Proxies
// GET Proxy URL
// Retrieves the location of a Proxy from ShareBrowser's database.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/preview/proxy_url?file_id={file_id}
// cURL Example:
// # Make sure to specify your session and send a GET request with the volume_id and path.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/preview/proxy_url?file_id=23"
// The above command returns JSON structured like this:
// 
// {
//    "status":"success",
//    "data":{
//       "video_proxy_url":"http://127.0.0.1/sb-api/api/v1.1/proxyservice/videoproxy/107021146/v23-1.mp4"
//    }
// }
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// GET Thumbnail URL
// Retrieves the location of a thumbnail from ShareBrowser's database.
// 
// HTTP Request:
// GET http://evo-address/sb-api/api/public/v1.0/preview/thumbnail_url?file_id={file_id}
// cURL Example:
// # Make sure to specify your session and send a GET request with the volume_id and path.
// curl -H "session_id: 1447413700" -X GET "http://127.0.0.1/sb-api/api/public/v1.0/preview/thumbnail_url?file_id=23"
// The above command returns JSON structured like this:
// 
// {
//    "status":"success",
//    "data":{
//       "thumbnail_url":"http://127.0.0.1/sb-api/api/v1.1/proxyservice/thumbnail/1607986310/t23-1.jpg"
//    }
// }
// Query Parameters:
// file_id	 The file_id returned from GET file_id. 
//  
// 
// Errors
// The ShareBrowser API uses the following error codes:
// 
// Error Code
//  Description 
// 1	Internal Server Error
// 3	Invalid or empty parameters
// 108	Volume Not Found
// 112	Invalid Username or Password
// 118	Invalid Session ID
// 121	Access Not Permitted
// 131	File not found
// 132	Thumbnail not found
// 133	Proxy not found
