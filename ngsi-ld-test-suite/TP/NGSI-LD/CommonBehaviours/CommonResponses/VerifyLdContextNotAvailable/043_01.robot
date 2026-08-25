*** Settings ***
Documentation       Verify receiving 504 – LdContextNotAvailable error if remote JSON-LD @context cannot be retrieved

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${expected_status_code}=        504
${building_filename}=           building-unretrievable-context.jsonld
${subscription_filename}=       subscriptions/subscription-unretrievable-context.jsonld
${tea_filename}=                bus-temporal-representation-unretrievable-context.jsonld
${registration_filename}=       csourceRegistrations/context-source-registration-unretrievable-context.jsonld


*** Test Cases ***
043_01_01 Create Entity
    [Documentation]    Verify receiving 504 – LdContextNotAvailable error if remote JSON-LD @context cannot be retrieved (Create entity)
    [Tags]    e-create    cb-ldcontext    5_2_2
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    [Teardown]    Delete Entity    ${entity_id}

043_01_02 Create Subscription
    [Documentation]    Verify receiving 504 – LdContextNotAvailable error if remote JSON-LD @context cannot be retrieved (Create subscription)
    [Tags]    sub-create    cb-ldcontext    5_2_2
    ${subscription_id}=    Generate Random Subscription Id
    ${response}=    Create Subscription    ${subscription_id}    ${subscription_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    [Teardown]    Delete Subscription    ${subscription_id}

043_01_03 Create Temporal Representation Of Entities
    [Documentation]    Verify receiving 504 – LdContextNotAvailable error if remote JSON-LD @context cannot be retrieved (Create Temporal Representation of Entities)
    [Tags]    te-create    cb-ldcontext    5_2_2
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    filename=${tea_filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    [Teardown]    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}

043_01_04 Batch Entity Create
    [Documentation]    Verify receiving 504 – LdContextNotAvailable error if remote JSON-LD @context cannot be retrieved (Batch entity create)
    [Tags]    be-create    cb-ldcontext    5_2_2
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Building Entity Id
    ${first_entity}=    Load Entity    ${building_filename}    ${first_entity_id}
    ${second_entity}=    Load Entity    ${building_filename}    ${second_entity_id}
    @{entities_to_be_created}=    Create List    ${first_entity}    ${second_entity}
    @{entities_ids_to_be_created}=    Create List    ${first_entity_id}    ${second_entity_id}
    ${response}=    Batch Create Entities    @{entities_to_be_created}    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    207    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()['errors'][0]['error']}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()['errors'][1]['error']}
    ...    ${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}
    Check Response Body Containing ProblemDetails Element Containing Title Element
    ...    ${response.json()['errors'][0]['error']}
    Check Response Body Containing ProblemDetails Element Containing Title Element
    ...    ${response.json()['errors'][1]['error']}
    [Teardown]    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_created}

043_01_05 Create Context Source Registration
    [Documentation]    Verify receiving 504 – LdContextNotAvailable error if remote JSON-LD @context cannot be retrieved (Create context source registration)
    [Tags]    csr-create    cb-ldcontext    5_2_2
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${registration_filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    [Teardown]    Delete Context Source Registration With Return    ${registration_id}
