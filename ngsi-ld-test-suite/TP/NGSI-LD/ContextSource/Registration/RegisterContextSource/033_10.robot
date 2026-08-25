*** Settings ***
Documentation       Check that one cannot create a context source with invalid content

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Context Source With Invalid Content


*** Test Cases ***
033_10_01 Create A Context Source Registration With A Different Data Structure Than CSourceRegistration Data Type
    [Tags]    csr-create    5_9_2
    csourceRegistrations/context-source-registration-invalid-structure.jsonld
033_10_02 Create A Context Source Registration With A Date In The Past
    [Tags]    csr-create    5_9_2
    csourceRegistrations/context-source-registration-past-expiration.jsonld


*** Keywords ***
Create Context Source With Invalid Content
    [Documentation]    Check that one cannot create a context source with invalid content
    [Arguments]    ${filename}
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Headers Containing URI set to    ${registration_id}    ${response.headers}
