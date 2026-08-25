*** Settings ***
Documentation       Check that one cannot delete a context source registration under some conditions

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Context Source Registration
Test Teardown       Delete Created Context Source Registrations
Test Template       Delete A Context Source


*** Variables ***
${filename}=    context-source-registration.jsonld


*** Test Cases ***    INVALID_REGISTRATION_ID    EXPECTED_STATUS_CODE    PROBLEM_TYPE
035_02_01 Delete A Context Source Registration If The Id Is Not Present
    [Tags]    csr-delete    5_9_4
    ${EMPTY}    405    ${EMPTY}
035_02_02 Delete A Context Source Registration If The Id Is Not A Valid URI
    [Tags]    csr-delete    5_9_4
    invalidURI    400    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Delete A Context Source
    [Documentation]    Check that one cannot delete a context source registration under some conditions
    [Arguments]    ${invalid_registration_id}    ${expected_status_code}    ${problem_type}
    ${response}=    Delete Context Source Registration With Return    ${invalid_registration_id}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    "${problem_type}"!="${EMPTY}"
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${problem_type}
        Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    END

Delete Created Context Source Registrations
    Delete Context Source Registration    ${registration_id}

Create Initial Context Source Registration
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${response.status_code}
