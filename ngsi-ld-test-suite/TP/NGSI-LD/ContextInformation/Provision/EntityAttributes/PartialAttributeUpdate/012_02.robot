*** Settings ***
Documentation       Check that one cannot perform a partial update on an entity attribute with invalid data

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Update Attributes


*** Variables ***
${filename}=        vehicle-two-datasetid-attributes.jsonld
${status_code}=     400


*** Test Cases ***    ENTITY_ID    ATTRIBUTE_ID    FRAGMENT_FILENAME
012_02_01 Make A Partial Attribute Update If The Entity Id Is Not Present
    [Documentation]    Check that one cannot perform a partial update on an entity attribute with missing entity id
    ${EMPTY}    speed    vehicle-speed-equal-datasetid-fragment.jsonld
012_02_02 Make A Partial Attribute Update If The Entity Id Is Not A Valid URI
    [Documentation]    Check that one cannot perform a partial update on an entity attribute with invalid entity id
    thisisaninvaliduri    speed    vehicle-speed-equal-datasetid-fragment.jsonld
012_02_03 Make A Partial Attribute Update If The Attribute Type Does Not Match
    [Documentation]    Check that one cannot perform a partial update on an entity attribute with a different attribute type
    ${valid_entity_id}    speed    vehicle-speed-equal-datasetid-different-type-fragment.jsonld
012_02_04 Make A Partial Attribute Update If The Entity Fragment Is Empty
    [Documentation]    Check that one cannot perform a partial update on an entity attribute with an empty fragment
    ${valid_entity_id}    speed    empty-fragment.json


*** Keywords ***
Update Attributes
    [Documentation]    Check that one cannot perform a partial update on an entity attribute in some conditions
    [Tags]    ea-partial-update    5_6_4
    [Arguments]    ${entity_id}    ${attribute_id}    ${fragment_filename}
    ${response}=    Partial Update Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    fragment_filename=${fragment_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Setup Initial Entity
    ${valid_entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${valid_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${valid_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${valid_entity_id}
