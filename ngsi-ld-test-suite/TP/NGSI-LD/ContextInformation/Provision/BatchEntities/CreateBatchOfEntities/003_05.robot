*** Settings ***
Documentation       Check that the default @context is used if the Content-Type header is "application/json" and the Link header does not contain a JSON-LD @context

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entities


*** Test Cases ***
003_05_01 Create A Batch Of One Entity Using The Default Context With JSON Content Type
    [Documentation]    Check that the default @context is used if the Content-Type header is "application/json" and the Link header does not contain a JSON-LD @context and retrieve the entity without context
    [Tags]    be-create    6_3_5

    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${entity}=    Load Entity    building-simple-attributes.json    ${entity_id}
    @{entities_to_be_created}=    Create List    ${entity}

    ${response}=    Batch Create Entities    @{entities_to_be_created}    content_type=${CONTENT_TYPE_JSON}

    Check Response Status Code    201    ${response.status_code}
    ${response1}=    Retrieve Entity    id=${entity_id}
    # Attribute should be compacted as one used the same default context as provided when creating the entity
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=almostFull
    ...    response_body=${response1.json()}

003_05_02 Create A Batch Of One Entity Using The Default Context With JSON Content Type
    [Documentation]    Check that the default @context is used if the Content-Type header is "application/json" and the Link header does not contain a JSON-LD @context and retrieve the entity with context
    [Tags]    be-create    6_3_5

    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${entity}=    Load Entity    building-simple-attributes.json    ${entity_id}
    @{entities_to_be_created}=    Create List    ${entity}

    ${response}=    Batch Create Entities    @{entities_to_be_created}    content_type=${CONTENT_TYPE_JSON}

    Check Response Status Code    201    ${response.status_code}
    ${response1}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    # Attribute should not be compacted as one did not provide a context containing this term
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=ngsi-ld:default-context/almostFull
    ...    response_body=${response1.json()}


*** Keywords ***
Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List    ${entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
