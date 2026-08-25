*** Settings ***
Documentation       Check that one cannot delete a context source registration by id if the id is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${registration_payload_file_path}=      context-source-registration-simple.jsonld


*** Test Cases ***
035_03_01 Delete A Context Source Registration By Id
    [Documentation]    Check that one cannot delete a context source registration by id if the id is not known to the system
    [Tags]    csr-delete    5_9_4
    ${registration_id}=    Generate Random CSR Id
    ${response}=    Delete Context Source Registration With Return    ${registration_id}
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
