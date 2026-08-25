*** Settings ***
Documentation       Check that one cannot create a temporal entity with an empty/invalid json/id

Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Create Temporal Entity


*** Variables ***
${status_code}=     400


*** Test Cases ***    ENTITY_ID    FILENAME
007_03_01 Create A Temporal Entity With Missing Id
    ${EMPTY}    vehicle-temporal-representation-without-id.jsonld
007_03_02 Create A Temporal Invalid URI
    invalidId    vehicle-temporal-representation.jsonld


*** Keywords ***
Create Temporal Entity
    [Documentation]    Check that one cannot create a temporal entity with an invalid @context
    [Tags]    te-create    5_6_11
    [Arguments]    ${entity_id}    ${filename}
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${entity_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    [Teardown]    Delete Temporal Representation Of Entity    ${entity_id}
