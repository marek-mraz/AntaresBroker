*** Settings ***
Documentation       Check that one cannot create a temporal entity with an empty/invalid json/id

Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Temporal Entity From File


*** Variables ***
${status_code}=     400


*** Test Cases ***    FILENAME
007_02_01 Create A Temporal Entity With An Invalid Json
    vehicle-temporal-representation-invalid-json.jsonld
007_02_02 Create A Temporal Entity With An Empty Json
    vehicle-temporal-representation-empty-json.jsonld


*** Keywords ***
Create Temporal Entity From File
    [Documentation]    Check that one cannot create a temporal entity with an empty/invalid json/id
    [Tags]    te-create    5_6_11
    [Arguments]    ${filename}
    ${response}=    Create Temporal Representation Of Entity Selecting Content Type
    ...    ${filename}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
