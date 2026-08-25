*** Settings ***
Documentation       Check that one cannot create a context source registration that already exists

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create New Context Source Registration
Test Teardown       Delete Created Context Source Registrations


*** Variables ***
${filename}=    csourceRegistrations/context-source-registration.jsonld


*** Test Cases ***
033_03_01 Create A Context Source Registration That Already Exists
    [Documentation]    Check that one cannot create a context source registration that already exists
    [Tags]    csr-create    5_9_2
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    409    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Delete Created Context Source Registrations
    Delete Context Source Registration    ${registration_id}

Create New Context Source Registration
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${updated_payload}
