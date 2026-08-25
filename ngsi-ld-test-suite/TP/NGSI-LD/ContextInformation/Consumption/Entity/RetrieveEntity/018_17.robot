*** Settings ***
Documentation       Check that one can retrieve an entity with pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Created Entity
Test Template       Retrieve Entity With Pick Or Omit Query Params


*** Variables ***
${filename}=    building-simple-attributes.jsonld


*** Test Cases ***    PICK    OMIT    EXPECTATION_FILENAME
018_17_01 RetrieveWithPickOnAttributes
    [Documentation]    Check that one can retrieve an entity with pick on attributes
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    name    ${EMPTY}    pick-omit/building-simple-attributes-pick-name.json
018_17_02 RetrieveWithOmitOnAttributes
    [Documentation]    Check that one can retrieve an entity with omit on attributes
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    ${EMPTY}    name    pick-omit/building-simple-attributes-omit-name.json
018_17_03 RetrieveWithPickOnCoreMembers
    [Documentation]    Check that one can retrieve an entity with pick on core members
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id    ${EMPTY}    pick-omit/building-simple-attributes-pick-id.json
018_17_04 RetrieveWithOmitOnCoreMembers
    [Documentation]    Check that one can retrieve an entity with omit on core members
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    ${EMPTY}    id    pick-omit/building-simple-attributes-omit-id.json
018_17_05 RetrieveWithPickOnAttributesAndCoreMembers
    [Documentation]    Check that one can retrieve an entity with pick on attributes and core members
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,name    ${EMPTY}    pick-omit/building-simple-attributes-pick-id-name.json
018_17_06 RetrieveWithOmitOnAttributesAndCoreMembers
    [Documentation]    Check that one can retrieve an entity with omit on attributes and core members
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    ${EMPTY}    type,name    pick-omit/building-simple-attributes-omit-type-name.json


*** Keywords ***
Retrieve Entity With Pick Or Omit Query Params
    [Documentation]    Retrieve an entity giving pick or omit query params different values
    [Arguments]    ${pick}    ${omit}    ${expectation_filename}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    pick=${pick}
    ...    omit=${omit}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element
    ...    ${expectation_filename}
    ...    ${entity_id}
    ...    ${response.json()}
    ...    ${True}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
